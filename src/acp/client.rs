//! ACP client implementation for Writ.
//!
//! The ACP library requires a single-threaded async runtime with local task support.
//! We run it in a dedicated thread with tokio's LocalSet and communicate via channels.

use agent_client_protocol::{
    Agent, Client, ClientSideConnection, ContentBlock, Implementation, InitializeRequest,
    NewSessionRequest, PermissionOptionKind, PromptRequest, ReadTextFileRequest,
    ReadTextFileResponse, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SelectedPermissionOutcome, SessionId, SessionNotification,
    SessionUpdate, TextContent, WriteTextFileRequest, WriteTextFileResponse,
};
use futures::future::LocalBoxFuture;
use std::path::PathBuf;
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
    /// Agent wants to write a file.
    WriteFile { path: PathBuf, content: String },
    /// Connection error or agent process exited.
    Error(String),
}

/// Commands sent to the ACP worker thread.
#[derive(Debug)]
pub enum AcpCommand {
    /// Send a prompt to the agent.
    Prompt(String),
    /// Shutdown the connection.
    Shutdown,
}

/// Handler that receives ACP callbacks and forwards them as events.
struct ClientHandler {
    event_tx: mpsc::UnboundedSender<AcpEvent>,
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
        eprintln!("[ACP] session_notification: {:?}", notification.update);
        match notification.update {
            SessionUpdate::AgentMessageChunk(chunk) => {
                if let ContentBlock::Text(TextContent { text, .. }) = chunk.content {
                    eprintln!("[ACP] AgentMessageChunk: {:?}", text);
                    let _ = self.event_tx.send(AcpEvent::AgentMessageChunk(text));
                }
            }
            _ => {
                // Ignore other updates for now (tool calls, thoughts, etc.)
                eprintln!("[ACP] Ignoring update type");
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
}

impl AcpClientHandle {
    /// Spawn an agent subprocess and establish an ACP connection.
    ///
    /// This runs the ACP protocol in a dedicated thread with its own tokio runtime.
    pub fn spawn(agent_command: String, cwd: PathBuf) -> Self {
        let (event_tx, event_rx) = std_mpsc::channel();
        let (command_tx, command_rx) = std_mpsc::channel();

        let thread_handle = thread::spawn(move || {
            let rt = Runtime::new().expect("Failed to create tokio runtime");
            let local = LocalSet::new();

            local.block_on(&rt, async move {
                if let Err(e) =
                    run_acp_worker(agent_command, cwd, event_tx.clone(), command_rx).await
                {
                    let _ = event_tx.send(AcpEvent::Error(e.to_string()));
                }
            });
        });

        Self {
            event_rx,
            command_tx,
            thread_handle,
        }
    }

    /// Send a prompt to the agent.
    pub fn send_prompt(&self, text: String) -> Result<(), std_mpsc::SendError<AcpCommand>> {
        self.command_tx.send(AcpCommand::Prompt(text))
    }

    /// Try to receive the next event, non-blocking.
    pub fn try_recv(&self) -> Option<AcpEvent> {
        self.event_rx.try_recv().ok()
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
    };

    // Wrap tokio types with futures-compatible wrappers
    let reader = tokio_util::compat::TokioAsyncReadCompatExt::compat(stdout);
    let writer = tokio_util::compat::TokioAsyncWriteCompatExt::compat_write(stdin);

    let spawn_fn = |fut: LocalBoxFuture<'static, ()>| {
        tokio::task::spawn_local(fut);
    };

    let (connection, io_future) = ClientSideConnection::new(handler, writer, reader, spawn_fn);

    // Spawn the I/O handler
    tokio::task::spawn_local(async move {
        if let Err(e) = io_future.await {
            eprintln!("ACP I/O error: {}", e);
        }
    });

    // Initialize the connection
    {
        use agent_client_protocol::ProtocolVersion;
        eprintln!("[ACP] Sending initialize request...");
        let request = InitializeRequest::new(ProtocolVersion::LATEST).client_info(
            Implementation::new("writ".to_string(), env!("CARGO_PKG_VERSION").to_string()),
        );
        let response = connection.initialize(request).await?;
        eprintln!("[ACP] Initialize response: {:?}", response);
    }

    // Create a session
    let session_id: SessionId = {
        eprintln!("[ACP] Creating new session in {:?}...", cwd);
        let request = NewSessionRequest::new(cwd);
        let response = connection.new_session(request).await?;
        eprintln!("[ACP] Session created: {:?}", response.session_id);
        response.session_id
    };

    // Signal that we're ready
    eprintln!("[ACP] Ready!");
    let _ = event_tx.send(AcpEvent::Ready);

    // Main event loop - poll both command channel and ACP events
    loop {
        tokio::select! {
            // Handle ACP events from the connection
            Some(acp_event) = async_event_rx.recv() => {
                if event_tx.send(acp_event).is_err() {
                    // Receiver dropped, exit
                    break;
                }
            }

            // Check for commands (non-blocking since std_mpsc isn't async)
            _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => {
                match command_rx.try_recv() {
                    Ok(AcpCommand::Prompt(text)) => {
                        eprintln!("[ACP] Sending prompt: {:?}", text);
                        let request = PromptRequest::new(
                            session_id.clone(),
                            vec![ContentBlock::from(text)],
                        );
                        match connection.prompt(request).await {
                            Ok(response) => {
                                eprintln!("[ACP] Prompt response: {:?}", response);
                                // Response complete
                                let _ = event_tx.send(AcpEvent::ResponseComplete);
                            }
                            Err(e) => {
                                eprintln!("[ACP] Prompt error: {:?}", e);
                                let _ = event_tx.send(AcpEvent::Error(e.to_string()));
                            }
                        }
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

    Ok(())
}
