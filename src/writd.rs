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

use writ::config::GITHUB_TOKEN_ENV;

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
    url: Option<String>,
    text: String,
    #[allow(dead_code)]
    selections: Option<Vec<Selection>>,
}

/// Extract owner/repo from a GitHub URL.
/// Handles URLs like:
/// - https://github.com/owner/repo/issues/123
/// - https://github.com/owner/repo/pull/456
/// - https://github.com/owner/repo
fn parse_github_repo_from_url(url: &str) -> Option<String> {
    let url = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))?;

    let mut parts = url.split('/');
    let owner = parts.next()?;
    let repo = parts.next()?;

    if owner.is_empty() || repo.is_empty() {
        return None;
    }

    Some(format!("{}/{}", owner, repo))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_github_repo_from_url() {
        // Issue URL
        assert_eq!(
            parse_github_repo_from_url("https://github.com/owner/repo/issues/123"),
            Some("owner/repo".to_string())
        );

        // PR URL
        assert_eq!(
            parse_github_repo_from_url("https://github.com/owner/repo/pull/456"),
            Some("owner/repo".to_string())
        );

        // Repo root
        assert_eq!(
            parse_github_repo_from_url("https://github.com/owner/repo"),
            Some("owner/repo".to_string())
        );

        // Non-GitHub URL
        assert_eq!(
            parse_github_repo_from_url("https://gitlab.com/owner/repo"),
            None
        );

        // Invalid URL
        assert_eq!(parse_github_repo_from_url("not a url"), None);
    }
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
    let mut buf = [0u8; 1024];
    let n = stream.peek(&mut buf).await?;
    let request = String::from_utf8_lossy(&buf[..n]);

    if request.contains("Upgrade: websocket") || request.contains("upgrade: websocket") {
        let ws_stream = tokio_tungstenite::accept_async(stream).await?;
        handle_websocket(ws_stream, peer_addr).await?;
    } else {
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

    let (file_tx, mut file_rx) = mpsc::channel::<String>(16);
    let watch_path = temp_file.clone();

    let mut watcher = notify::recommended_watcher(move |res: Result<notify::Event, _>| {
        if let Ok(event) = res
            && matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_))
            && let Ok(content) = std::fs::read_to_string(&watch_path)
        {
            let _ = file_tx.blocking_send(content);
        }
    })?;

    watcher.watch(&temp_file, RecursiveMode::NonRecursive)?;

    // Build writ command with optional GitHub context
    let mut cmd = Command::new("writ");
    cmd.arg("--file").arg(&temp_file).arg("--autosave");

    // Pass GitHub repo context if URL is a GitHub page
    if let Some(ref url) = client_msg.url
        && let Some(repo) = parse_github_repo_from_url(url)
    {
        cmd.arg("--github-repo").arg(&repo);
        println!("Detected GitHub repo: {}", repo);
    }

    // Pass GitHub token if available in environment
    if let Ok(token) = std::env::var(GITHUB_TOKEN_ENV) {
        cmd.arg("--github-token").arg(token);
    }

    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    println!("Spawned writ process for {:?}", temp_file);

    let mut last_content = client_msg.text.clone();

    loop {
        tokio::select! {
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

            Some(msg) = read.next() => {
                match msg {
                    Ok(Message::Text(text)) => {
                        if let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text)
                            && client_msg.text != last_content
                        {
                            last_content = client_msg.text.clone();
                            std::fs::write(&temp_file, &client_msg.text)?;
                        }
                    }
                    Ok(Message::Close(_)) => break,
                    Err(_) => break,
                    _ => {}
                }
            }

            status = child.wait() => {
                println!("writ process exited with {:?} for {:?}", status, temp_file);
                if let Ok(content) = std::fs::read_to_string(&temp_file)
                    && content != last_content
                {
                    let msg = ServerMessage {
                        text: content,
                        selections: None,
                    };
                    let json = serde_json::to_string(&msg)?;
                    let _ = write.send(Message::Text(json.into())).await;
                }
                break;
            }
        }
    }

    drop(watcher);
    let _ = std::fs::remove_file(&temp_file);
    println!("Cleaned up session for {}", peer_addr);

    Ok(())
}
