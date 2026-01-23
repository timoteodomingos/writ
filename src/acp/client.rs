//! ACP client implementation for Writ.
//!
//! Uses GPUI's async executor to run the ACP protocol.

use agent_client_protocol::{
    Agent, CancelNotification, Client, ClientCapabilities, ClientSideConnection, ContentBlock,
    FileSystemCapability, Implementation, InitializeRequest, NewSessionRequest,
    PermissionOptionKind, PromptRequest, ReadTextFileRequest, ReadTextFileResponse,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse, ResourceLink,
    SelectedPermissionOutcome, SessionId, SessionNotification, SessionUpdate, TextContent,
    WriteTextFileRequest, WriteTextFileResponse,
};
use async_process::{Child, Command, Stdio};
use gpui::{AppContext, Context, Entity, Task};
use smol::channel;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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

/// Handler that receives ACP callbacks and forwards them as events.
struct ClientHandler {
    event_tx: channel::Sender<AcpEvent>,
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
                    let _ = self.event_tx.send(AcpEvent::AgentMessageChunk(text)).await;
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
        let _ = self
            .event_tx
            .send(AcpEvent::WriteFile {
                path: request.path.clone(),
                content: request.content.clone(),
            })
            .await;
        Ok(WriteTextFileResponse::new())
    }
}

/// ACP client state managed as a GPUI entity.
pub struct AcpClient {
    /// Channel to receive events from the ACP connection.
    event_rx: channel::Receiver<AcpEvent>,
    /// Channel to send events (for internal use like ResponseComplete).
    event_tx: channel::Sender<AcpEvent>,
    /// The ACP connection (wrapped for sharing with spawned tasks).
    connection: Rc<RefCell<Option<ClientSideConnection>>>,
    /// Current session ID.
    session_id: Rc<RefCell<Option<SessionId>>>,
    /// Shared flag indicating a write event is pending.
    pending_write: Arc<AtomicBool>,
    /// Handle to the I/O task.
    #[allow(dead_code)]
    io_task: Task<()>,
    /// Handle to the child process.
    child: Rc<RefCell<Option<Child>>>,
}

impl AcpClient {
    /// Spawn an agent subprocess and establish an ACP connection.
    pub fn new<V: 'static>(
        agent_command: String,
        cwd: PathBuf,
        cx: &mut Context<V>,
    ) -> Entity<Self> {
        cx.new(|cx: &mut Context<Self>| {
            let (event_tx, event_rx) = channel::unbounded::<AcpEvent>();
            let pending_write = Arc::new(AtomicBool::new(false));
            let connection: Rc<RefCell<Option<ClientSideConnection>>> = Rc::new(RefCell::new(None));
            let session_id: Rc<RefCell<Option<SessionId>>> = Rc::new(RefCell::new(None));
            let child: Rc<RefCell<Option<Child>>> = Rc::new(RefCell::new(None));

            // Spawn the connection setup task
            let io_task = {
                let connection = Rc::clone(&connection);
                let session_id = Rc::clone(&session_id);
                let child_rc = Rc::clone(&child);
                let pending_write = Arc::clone(&pending_write);
                let event_tx_clone = event_tx.clone();
                let foreground = cx.foreground_executor().clone();

                cx.spawn(async move |_, _cx| {
                    if let Err(e) = Self::setup_connection(
                        agent_command,
                        cwd,
                        connection,
                        session_id,
                        child_rc,
                        pending_write,
                        event_tx_clone.clone(),
                        foreground,
                    )
                    .await
                    {
                        let _ = event_tx_clone.send(AcpEvent::Error(e.to_string())).await;
                    }
                })
            };

            Self {
                event_rx,
                event_tx,
                connection,
                session_id,
                pending_write,
                io_task,
                child,
            }
        })
    }

    async fn setup_connection(
        agent_command: String,
        cwd: PathBuf,
        connection_rc: Rc<RefCell<Option<ClientSideConnection>>>,
        session_id_rc: Rc<RefCell<Option<SessionId>>>,
        child_rc: Rc<RefCell<Option<Child>>>,
        pending_write: Arc<AtomicBool>,
        event_tx: channel::Sender<AcpEvent>,
        foreground: gpui::ForegroundExecutor,
    ) -> anyhow::Result<()> {
        // Spawn the agent subprocess
        let mut child = Command::new(&agent_command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;

        let stdin = child.stdin.take().expect("stdin should be available");
        let stdout = child.stdout.take().expect("stdout should be available");

        // Store the child process handle
        *child_rc.borrow_mut() = Some(child);

        let handler = ClientHandler {
            event_tx: event_tx.clone(),
            pending_write,
        };

        // Create the ACP connection using GPUI's foreground executor for spawning
        let (conn, io_future) = ClientSideConnection::new(handler, stdin, stdout, move |fut| {
            foreground.spawn(fut).detach();
        });

        // Store the connection
        *connection_rc.borrow_mut() = Some(conn);

        // Run the I/O handler in the background
        smol::spawn(async move {
            if let Err(e) = io_future.await {
                eprintln!("ACP I/O error: {}", e);
            }
        })
        .detach();

        // Initialize the connection
        {
            use agent_client_protocol::ProtocolVersion;

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

            if let Some(ref conn) = *connection_rc.borrow() {
                conn.initialize(request).await?;
            }
        }

        // Create a session
        {
            let request = NewSessionRequest::new(cwd);
            if let Some(ref conn) = *connection_rc.borrow() {
                let response = conn.new_session(request).await?;
                *session_id_rc.borrow_mut() = Some(response.session_id);
            }
        }

        // Signal that we're ready
        let _ = event_tx.send(AcpEvent::Ready).await;

        Ok(())
    }

    /// Send a prompt to the agent.
    pub fn send_prompt(&self, text: String, file_path: Option<PathBuf>, cx: &mut Context<Self>) {
        let connection = Rc::clone(&self.connection);
        let session_id = self.session_id.borrow().clone();
        let event_tx = self.event_tx.clone();

        if let Some(session_id) = session_id {
            cx.spawn(async move |_, _| {
                // Build prompt content blocks
                let mut content_blocks: Vec<ContentBlock> = Vec::new();

                if let Some(ref path) = file_path {
                    let uri = format!("file://{}", path.display());
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "file".to_string());
                    content_blocks.push(ContentBlock::ResourceLink(ResourceLink::new(name, uri)));
                }

                content_blocks.push(ContentBlock::Text(TextContent::new(text)));

                let request = PromptRequest::new(session_id, content_blocks);

                let result = {
                    if let Some(ref conn) = *connection.borrow() {
                        Some(conn.prompt(request).await)
                    } else {
                        None
                    }
                };

                // Send completion event
                match result {
                    Some(Ok(_)) => {
                        let _ = event_tx.send(AcpEvent::ResponseComplete).await;
                    }
                    Some(Err(e)) => {
                        let _ = event_tx.send(AcpEvent::Error(e.to_string())).await;
                    }
                    None => {
                        let _ = event_tx
                            .send(AcpEvent::Error("No connection".to_string()))
                            .await;
                    }
                }
            })
            .detach();
        }
    }

    /// Cancel the current prompt.
    pub fn cancel(&self, cx: &mut Context<Self>) {
        let connection = Rc::clone(&self.connection);
        let session_id = self.session_id.borrow().clone();

        if let Some(session_id) = session_id {
            cx.spawn(async move |_, _| {
                if let Some(ref conn) = *connection.borrow() {
                    let _ = conn.cancel(CancelNotification::new(session_id)).await;
                }
            })
            .detach();
        }
    }

    /// Try to receive the next event, non-blocking.
    pub fn try_recv(&self) -> Option<AcpEvent> {
        self.event_rx.try_recv().ok()
    }

    /// Check if a write is pending.
    pub fn is_write_pending(&self) -> bool {
        self.pending_write.load(Ordering::SeqCst)
    }

    /// Clear the pending write flag.
    pub fn clear_pending_write(&self) {
        self.pending_write.store(false, Ordering::SeqCst);
    }
}

impl Drop for AcpClient {
    fn drop(&mut self) {
        // Kill the child process if still running
        if let Some(ref mut child) = *self.child.borrow_mut() {
            let _ = child.kill();
        }
    }
}
