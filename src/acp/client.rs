//! ACP client implementation for Writ.
//!
//! The ACP library requires a single-threaded async runtime with local task support.
//! We run it in a dedicated thread with tokio's LocalSet and communicate via channels.

use agent_client_protocol::{
    Agent, CancelNotification, Client, ClientCapabilities, ClientSideConnection, ContentBlock,
    FileSystemCapability, Implementation, InitializeRequest, NewSessionRequest,
    PermissionOptionKind, PromptRequest, ReadTextFileRequest, ReadTextFileResponse,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse, ResourceLink,
    SelectedPermissionOutcome, SessionId, SessionNotification, SessionUpdate, TextContent,
    WriteTextFileRequest, WriteTextFileResponse,
};
use futures::future::LocalBoxFuture;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc as std_mpsc;
use std::thread;
use tokio::process::Command;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use tokio::task::LocalSet;

/// Events emitted by the ACP client for the UI to handle.
#[derive(Debug, Clone)]
pub enum AcpEvent {
    /// Connection established and ready.
    Ready,
    /// Agent is streaming a chunk of its response message.
    AgentMessageChunk(String),
    /// Agent response is complete.
    ResponseComplete,
    /// Agent wants to write a file (via fs/write_text_file RPC).
    WriteFile { path: PathBuf, content: String },
    /// Connection error or agent process exited.
    Error(String),
}

/// Commands sent to the ACP worker thread.
#[derive(Debug)]
pub enum AcpCommand {
    /// Send a prompt to the agent, with optional file context.
    Prompt {
        text: String,
        /// If set, prepend file context to the prompt.
        file_path: Option<PathBuf>,
    },
    /// Cancel the current prompt.
    Cancel,
    /// Shutdown the connection.
    Shutdown,
}

/// Handler that receives ACP callbacks and forwards them as events.
struct ClientHandler {
    event_tx: mpsc::UnboundedSender<AcpEvent>,
    /// Shared flag to signal a write is pending (blocks file watcher reload).
    pending_write: Arc<AtomicBool>,
}

#[async_trait::async_trait(?Send)]
impl Client for ClientHandler {
    async fn request_permission(
        &self,
        request: RequestPermissionRequest,
    ) -> agent_client_protocol::Result<RequestPermissionResponse> {
        // Auto-select the first "allow once" or "allow always" option
        let option = request
            .options
            .iter()
            .find(|o| {
                matches!(
                    o.kind,
                    PermissionOptionKind::AllowOnce | PermissionOptionKind::AllowAlways
                )
            })
            .or_else(|| request.options.first());

        match option {
            Some(opt) => Ok(RequestPermissionResponse::new(
                RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                    opt.option_id.clone(),
                )),
            )),
            None => Ok(RequestPermissionResponse::new(
                RequestPermissionOutcome::Cancelled,
            )),
        }
    }

    async fn session_notification(
        &self,
        notification: SessionNotification,
    ) -> agent_client_protocol::Result<()> {
        match notification.update {
            SessionUpdate::AgentMessageChunk(chunk) => {
                if let ContentBlock::Text(TextContent { text, .. }) = chunk.content {
                    let _ = self.event_tx.send(AcpEvent::AgentMessageChunk(text));
                }
            }
            SessionUpdate::ToolCall(_) => {
                // ToolCall notifications with Diff content are informational only.
                // The actual file write comes through write_text_file RPC since we
                // advertise filesystem capabilities.
            }
            _ => {
                // Ignore other updates (tool call updates, thoughts, etc.)
            }
        }
        Ok(())
    }

    async fn read_text_file(
        &self,
        request: ReadTextFileRequest,
    ) -> agent_client_protocol::Result<ReadTextFileResponse> {
        // Read from disk for now - we'll route through editor buffer later
        match std::fs::read_to_string(&request.path) {
            Ok(content) => Ok(ReadTextFileResponse::new(content)),
            Err(e) => Err(agent_client_protocol::Error::new(
                -32000,
                format!("Failed to read file: {}", e),
            )),
        }
    }

    async fn write_text_file(
        &self,
        request: WriteTextFileRequest,
    ) -> agent_client_protocol::Result<WriteTextFileResponse> {
        // Set pending write flag BEFORE sending event to prevent file watcher race
        self.pending_write.store(true, Ordering::SeqCst);
        // Don't actually write - send event for the UI to handle through the diff system
        let _ = self.event_tx.send(AcpEvent::WriteFile {
            path: request.path.clone(),
            content: request.content.clone(),
        });
        Ok(WriteTextFileResponse::new())
    }
}

/// Handle to an ACP client running in a background thread.
pub struct AcpClientHandle {
    /// Receiver for events from the ACP connection.
    event_rx: std_mpsc::Receiver<AcpEvent>,
    /// Sender for commands to the ACP worker.
    command_tx: std_mpsc::Sender<AcpCommand>,
    /// Handle to the worker thread.
    #[allow(dead_code)]
    thread_handle: thread::JoinHandle<()>,
    /// Shared flag indicating a write event is pending.
    /// Used to prevent file watcher from reloading during the race window.
    pending_write: Arc<AtomicBool>,
}

impl AcpClientHandle {
    /// Spawn an agent subprocess and establish an ACP connection.
    ///
    /// This runs the ACP protocol in a dedicated thread with its own tokio runtime.
    pub fn spawn(agent_command: String, cwd: PathBuf) -> Self {
        let (event_tx, event_rx) = std_mpsc::channel();
        let (command_tx, command_rx) = std_mpsc::channel();
        let pending_write = Arc::new(AtomicBool::new(false));
        let pending_write_clone = pending_write.clone();

        let thread_handle = thread::spawn(move || {
            let rt = Runtime::new().expect("Failed to create tokio runtime");
            let local = LocalSet::new();

            local.block_on(&rt, async move {
                if let Err(e) = run_acp_worker(
                    agent_command,
                    cwd,
                    event_tx.clone(),
                    command_rx,
                    pending_write_clone,
                )
                .await
                {
                    let _ = event_tx.send(AcpEvent::Error(e.to_string()));
                }
            });
        });

        Self {
            event_rx,
            command_tx,
            thread_handle,
            pending_write,
        }
    }

    /// Send a prompt to the agent.
    pub fn send_prompt(
        &self,
        text: String,
        file_path: Option<PathBuf>,
    ) -> Result<(), std_mpsc::SendError<AcpCommand>> {
        self.command_tx.send(AcpCommand::Prompt { text, file_path })
    }

    /// Cancel the current prompt.
    pub fn cancel(&self) -> Result<(), std_mpsc::SendError<AcpCommand>> {
        self.command_tx.send(AcpCommand::Cancel)
    }

    /// Try to receive the next event, non-blocking.
    pub fn try_recv(&self) -> Option<AcpEvent> {
        self.event_rx.try_recv().ok()
    }

    /// Check if a write is pending (set when agent sends write, cleared after processing).
    pub fn is_write_pending(&self) -> bool {
        self.pending_write.load(Ordering::SeqCst)
    }

    /// Clear the pending write flag (call after processing WriteFile event).
    pub fn clear_pending_write(&self) {
        self.pending_write.store(false, Ordering::SeqCst);
    }

    /// Shutdown the connection.
    pub fn shutdown(&self) {
        let _ = self.command_tx.send(AcpCommand::Shutdown);
    }
}

impl Drop for AcpClientHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Run the ACP worker loop.
async fn run_acp_worker(
    agent_command: String,
    cwd: PathBuf,
    event_tx: std_mpsc::Sender<AcpEvent>,
    command_rx: std_mpsc::Receiver<AcpCommand>,
    pending_write: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    // Spawn the agent subprocess
    let mut child = Command::new(&agent_command)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()?;

    let stdin = child.stdin.take().expect("stdin should be available");
    let stdout = child.stdout.take().expect("stdout should be available");

    // Create channels for ACP events (async side)
    let (async_event_tx, mut async_event_rx) = mpsc::unbounded_channel();
    let handler = ClientHandler {
        event_tx: async_event_tx,
        pending_write,
    };

    // Wrap tokio types with futures-compatible wrappers
    let reader = tokio_util::compat::TokioAsyncReadCompatExt::compat(stdout);
    let writer = tokio_util::compat::TokioAsyncWriteCompatExt::compat_write(stdin);

    let spawn_fn = |fut: LocalBoxFuture<'static, ()>| {
        tokio::task::spawn_local(fut);
    };

    let (connection, io_future) = ClientSideConnection::new(handler, writer, reader, spawn_fn);

    // Wrap in Rc<RefCell<>> so we can share it with spawned tasks
    let connection = Rc::new(RefCell::new(connection));

    // Spawn the I/O handler
    tokio::task::spawn_local(async move {
        if let Err(e) = io_future.await {
            eprintln!("ACP I/O error: {}", e);
        }
    });

    // Initialize the connection
    {
        use agent_client_protocol::ProtocolVersion;

        // Advertise our filesystem capabilities so the agent uses our RPC methods
        // instead of writing directly to disk
        let fs_caps = FileSystemCapability::new()
            .read_text_file(true)
            .write_text_file(true);
        let client_caps = ClientCapabilities::new().fs(fs_caps);

        let request = InitializeRequest::new(ProtocolVersion::LATEST)
            .client_info(Implementation::new(
                "writ".to_string(),
                env!("CARGO_PKG_VERSION").to_string(),
            ))
            .client_capabilities(client_caps);
        connection.borrow().initialize(request).await?;
    }

    // Create a session
    let session_id: SessionId = {
        let request = NewSessionRequest::new(cwd);
        let response = connection.borrow().new_session(request).await?;
        response.session_id
    };

    // Signal that we're ready
    let _ = event_tx.send(AcpEvent::Ready);

    // Channel for prompt completion signals (so we don't block the event loop)
    let (prompt_done_tx, mut prompt_done_rx) = mpsc::unbounded_channel::<Result<(), String>>();

    // Main event loop - poll both command channel and ACP events
    loop {
        tokio::select! {
            // Handle ACP events from the connection (agent message chunks, etc.)
            Some(acp_event) = async_event_rx.recv() => {
                if event_tx.send(acp_event).is_err() {
                    // Receiver dropped, exit
                    break;
                }
            }

            // Handle prompt completion from spawned task
            Some(result) = prompt_done_rx.recv() => {
                match result {
                    Ok(()) => {
                        let _ = event_tx.send(AcpEvent::ResponseComplete);
                    }
                    Err(e) => {
                        let _ = event_tx.send(AcpEvent::Error(e));
                    }
                }
            }

            // Check for commands (non-blocking since std_mpsc isn't async)
            _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => {
                match command_rx.try_recv() {
                    Ok(AcpCommand::Prompt { text, file_path }) => {
                        // Build prompt content blocks
                        let mut content_blocks: Vec<ContentBlock> = Vec::new();

                        // Include file as ResourceLink if we have a path
                        if let Some(ref path) = file_path {
                            let uri = format!("file://{}", path.display());
                            let name = path
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_else(|| "file".to_string());
                            content_blocks
                                .push(ContentBlock::ResourceLink(ResourceLink::new(name, uri)));
                        }

                        // Add the user's text message
                        content_blocks.push(ContentBlock::Text(TextContent::new(text)));

                        let request = PromptRequest::new(session_id.clone(), content_blocks);

                        // Spawn the prompt call as a separate task so we don't block
                        // the event loop while waiting for the response. This allows
                        // agent message chunks to be processed as they arrive.
                        let connection_rc = Rc::clone(&connection);
                        let done_tx = prompt_done_tx.clone();
                        tokio::task::spawn_local(async move {
                            match connection_rc.borrow().prompt(request).await {
                                Ok(_) => {
                                    let _ = done_tx.send(Ok(()));
                                }
                                Err(e) => {
                                    let _ = done_tx.send(Err(e.to_string()));
                                }
                            }
                        });
                    }
                    Ok(AcpCommand::Cancel) => {
                        // Send cancel notification to stop the current prompt
                        let connection_rc = Rc::clone(&connection);
                        let sid = session_id.clone();
                        tokio::task::spawn_local(async move {
                            let _ = connection_rc
                                .borrow()
                                .cancel(CancelNotification::new(sid))
                                .await;
                        });
                    }
                    Ok(AcpCommand::Shutdown) => {
                        break;
                    }
                    Err(std_mpsc::TryRecvError::Empty) => {
                        // No commands, continue
                    }
                    Err(std_mpsc::TryRecvError::Disconnected) => {
                        // Command channel closed, exit
                        break;
                    }
                }
            }
        }
    }

    // Drop the connection to close stdin, signaling the agent to exit
    drop(connection);

    // Give the agent a moment to exit gracefully, then force kill if needed
    tokio::select! {
        _ = child.wait() => {}
        _ = tokio::time::sleep(std::time::Duration::from_secs(2)) => {
            let _ = child.kill().await;
        }
    }

    Ok(())
}
