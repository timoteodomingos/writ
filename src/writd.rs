//! writd - GhostText daemon for writ
//!
//! Listens on port 4001 for GhostText browser extension connections.
//! When a connection arrives, spawns a writ instance to edit the textarea content.

use std::net::SocketAddr;
use std::process::Stdio;

use anyhow::Result;
use futures::{SinkExt, StreamExt};
use notify::{EventKind, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

const DEFAULT_PORT: u16 = 4001;

/// GhostText protocol handshake response
#[derive(Serialize)]
struct HandshakeResponse {
    #[serde(rename = "ProtocolVersion")]
    protocol_version: u32,
    #[serde(rename = "WebSocketPort")]
    websocket_port: u16,
}

/// Message from browser to editor
#[derive(Deserialize, Debug)]
struct ClientMessage {
    title: Option<String>,
    #[allow(dead_code)]
    url: Option<String>,
    text: String,
    #[allow(dead_code)]
    selections: Option<Vec<Selection>>,
}

/// Message from editor to browser
#[derive(Serialize)]
struct ServerMessage {
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    selections: Option<Vec<Selection>>,
}

/// Selection/cursor position (UTF-16 code units)
#[derive(Deserialize, Serialize, Clone, Debug)]
struct Selection {
    start: usize,
    end: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    let port = DEFAULT_PORT;
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();

    let listener = TcpListener::bind(addr).await?;
    println!("writd listening on http://{}", addr);

    loop {
        let (stream, peer_addr) = listener.accept().await?;

        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, peer_addr, port).await {
                eprintln!("Connection error from {}: {}", peer_addr, e);
            }
        });
    }
}

async fn handle_connection(mut stream: TcpStream, peer_addr: SocketAddr, port: u16) -> Result<()> {
    // Read the HTTP request to determine if it's a WebSocket upgrade or regular GET
    let mut buf = [0u8; 1024];
    let n = stream.peek(&mut buf).await?;
    let request = String::from_utf8_lossy(&buf[..n]);

    if request.contains("Upgrade: websocket") || request.contains("upgrade: websocket") {
        // WebSocket upgrade - use tokio-tungstenite
        let ws_stream = tokio_tungstenite::accept_async(stream).await?;
        handle_websocket(ws_stream, peer_addr).await?;
    } else {
        // Regular HTTP GET - return handshake JSON
        // Consume the request first
        let mut request_buf = vec![0u8; 1024];
        let _ = stream.read(&mut request_buf).await?;

        let response = HandshakeResponse {
            protocol_version: 1,
            websocket_port: port,
        };
        let json = serde_json::to_string(&response)?;

        let http_response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            json.len(),
            json
        );

        stream.write_all(http_response.as_bytes()).await?;
    }

    Ok(())
}

async fn handle_websocket(
    ws_stream: tokio_tungstenite::WebSocketStream<TcpStream>,
    peer_addr: SocketAddr,
) -> Result<()> {
    let (mut write, mut read) = ws_stream.split();

    // Wait for initial message from browser
    let initial_msg = match read.next().await {
        Some(Ok(Message::Text(text))) => text,
        Some(Ok(msg)) => {
            eprintln!("Unexpected message type: {:?}", msg);
            return Ok(());
        }
        Some(Err(e)) => return Err(e.into()),
        None => return Ok(()),
    };

    let client_msg: ClientMessage = serde_json::from_str(&initial_msg)?;
    let title = client_msg
        .title
        .clone()
        .unwrap_or_else(|| "ghosttext".to_string());

    // Create temp file with content
    let temp_dir = std::env::temp_dir();
    let sanitized_title: String = title
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .take(50)
        .collect();
    let temp_file = temp_dir.join(format!(
        "ghosttext-{}-{}.md",
        sanitized_title,
        peer_addr.port()
    ));

    std::fs::write(&temp_file, &client_msg.text)?;
    println!("Created temp file: {:?}", temp_file);

    // Set up file watcher
    let (file_tx, mut file_rx) = mpsc::channel::<String>(16);
    let watch_path = temp_file.clone();

    let mut watcher = notify::recommended_watcher(move |res: Result<notify::Event, _>| {
        if let Ok(event) = res {
            if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                if let Ok(content) = std::fs::read_to_string(&watch_path) {
                    let _ = file_tx.blocking_send(content);
                }
            }
        }
    })?;

    watcher.watch(&temp_file, RecursiveMode::NonRecursive)?;

    // Spawn writ process
    let mut child = Command::new("writ")
        .arg("--file")
        .arg(&temp_file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    println!("Spawned writ process for {:?}", temp_file);

    // Track last content to avoid duplicate sends
    let mut last_content = client_msg.text.clone();

    // Main event loop
    loop {
        tokio::select! {
            // File changed - send update to browser
            Some(content) = file_rx.recv() => {
                if content != last_content {
                    last_content = content.clone();
                    let msg = ServerMessage {
                        text: content,
                        selections: None,
                    };
                    let json = serde_json::to_string(&msg)?;
                    if write.send(Message::Text(json.into())).await.is_err() {
                        break;
                    }
                }
            }

            // Browser sent update - write to file
            Some(msg) = read.next() => {
                match msg {
                    Ok(Message::Text(text)) => {
                        if let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) {
                            if client_msg.text != last_content {
                                last_content = client_msg.text.clone();
                                std::fs::write(&temp_file, &client_msg.text)?;
                            }
                        }
                    }
                    Ok(Message::Close(_)) => break,
                    Err(_) => break,
                    _ => {}
                }
            }

            // writ process exited
            status = child.wait() => {
                println!("writ process exited with {:?} for {:?}", status, temp_file);
                // Send final content before closing
                if let Ok(content) = std::fs::read_to_string(&temp_file) {
                    if content != last_content {
                        let msg = ServerMessage {
                            text: content,
                            selections: None,
                        };
                        let json = serde_json::to_string(&msg)?;
                        let _ = write.send(Message::Text(json.into())).await;
                    }
                }
                break;
            }
        }
    }

    // Cleanup
    drop(watcher);
    let _ = std::fs::remove_file(&temp_file);
    println!("Cleaned up session for {}", peer_addr);

    Ok(())
}
