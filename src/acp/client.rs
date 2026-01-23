//! ACP client implementation for Writ.
//!
//! Uses a channel-based design where a single long-running task owns the
//! connection and processes commands sequentially.

use agent_client_protocol::{
    Agent, CancelNotification, Client, ClientCapabilities, ClientSideConnection, ContentBlock,
    FileSystemCapability, Implementation, InitializeRequest, NewSessionRequest,
    PermissionOptionKind, PromptRequest, ReadTextFileRequest, ReadTextFileResponse,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse, ResourceLink,
    SelectedPermissionOutcome, SessionNotification, SessionUpdate, TextContent,
    WriteTextFileRequest, WriteTextFileResponse,
};
use async_process::{Command, Stdio};
use gpui::{AppContext, Context, Entity, Task};
use smol::channel;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Commands sent to the ACP connection task.
enum AcpCommand {
    /// Send a prompt to the agent.
    SendPrompt {
        text: String,
        file_path: Option<PathBuf>,
    },
    /// Cancel the current prompt.
    Cancel,
}

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
///
/// Uses a channel-based design: commands are sent to a single task that owns
/// the connection, and events are received from that task.
pub struct AcpClient {
    /// Channel to send commands to the connection task.
    command_tx: channel::Sender<AcpCommand>,
    /// Channel to receive events from the connection task.
    event_rx: channel::Receiver<AcpEvent>,
    /// Shared flag indicating a write event is pending.
    pending_write: Arc<AtomicBool>,
    /// Handle to the connection task (keeps it alive).
    #[allow(dead_code)]
    task: Task<()>,
}

impl AcpClient {
    /// Spawn an agent subprocess and establish an ACP connection.
    pub fn new<V: 'static>(
        agent_command: String,
        cwd: PathBuf,
        cx: &mut Context<V>,
    ) -> Entity<Self> {
        cx.new(|cx: &mut Context<Self>| {
            let (command_tx, command_rx) = channel::unbounded::<AcpCommand>();
            let (event_tx, event_rx) = channel::unbounded::<AcpEvent>();
            let pending_write = Arc::new(AtomicBool::new(false));

            let pending_write_clone = Arc::clone(&pending_write);
            let foreground = cx.foreground_executor().clone();

            // Spawn the main connection task
            let task = cx.spawn(async move |_, _cx| {
                if let Err(e) = run_connection(
                    agent_command,
                    cwd,
                    command_rx,
                    event_tx.clone(),
                    pending_write_clone,
                    foreground,
                )
                .await
                {
                    let _ = event_tx.send(AcpEvent::Error(e.to_string())).await;
                }
            });

            Self {
                command_tx,
                event_rx,
                pending_write,
                task,
            }
        })
    }

    /// Send a prompt to the agent.
    pub fn send_prompt(&self, text: String, file_path: Option<PathBuf>, _cx: &mut Context<Self>) {
        let _ = self
            .command_tx
            .try_send(AcpCommand::SendPrompt { text, file_path });
    }

    /// Cancel the current prompt.
    pub fn cancel(&self, _cx: &mut Context<Self>) {
        let _ = self.command_tx.try_send(AcpCommand::Cancel);
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

/// The main connection task that owns the ACP connection and processes commands.
async fn run_connection(
    agent_command: String,
    cwd: PathBuf,
    command_rx: channel::Receiver<AcpCommand>,
    event_tx: channel::Sender<AcpEvent>,
    pending_write: Arc<AtomicBool>,
    foreground: gpui::ForegroundExecutor,
) -> anyhow::Result<()> {
    // Spawn the agent subprocess
    let mut child = Command::new(&agent_command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()?;

    let stdin = child.stdin.take().expect("stdin should be available");
    let stdout = child.stdout.take().expect("stdout should be available");

    let handler = ClientHandler {
        event_tx: event_tx.clone(),
        pending_write,
    };

    // Create the ACP connection
    let (conn, io_future) = ClientSideConnection::new(handler, stdin, stdout, move |fut| {
        foreground.spawn(fut).detach();
    });

    // Run the I/O handler in the background
    smol::spawn(async move {
        if let Err(e) = io_future.await {
            eprintln!("ACP I/O error: {}", e);
        }
    })
    .detach();

    // Initialize the connection
    let session_id = {
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

        conn.initialize(request).await?;

        // Create a session
        let request = NewSessionRequest::new(cwd);
        let response = conn.new_session(request).await?;
        response.session_id
    };

    // Signal that we're ready
    let _ = event_tx.send(AcpEvent::Ready).await;

    // Process commands
    while let Ok(cmd) = command_rx.recv().await {
        match cmd {
            AcpCommand::SendPrompt { text, file_path } => {
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

                let request = PromptRequest::new(session_id.clone(), content_blocks);

                match conn.prompt(request).await {
                    Ok(_) => {
                        let _ = event_tx.send(AcpEvent::ResponseComplete).await;
                    }
                    Err(e) => {
                        let _ = event_tx.send(AcpEvent::Error(e.to_string())).await;
                    }
                }
            }
            AcpCommand::Cancel => {
                let _ = conn
                    .cancel(CancelNotification::new(session_id.clone()))
                    .await;
            }
        }
    }

    Ok(())
}
