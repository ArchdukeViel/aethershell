use crate::framing::{decode_response, encode_request, FramingError};
use crate::protocol::{IpcRequest, IpcResponse};
use std::io;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::ClientOptions;

/// Asynchronous Named Pipe IPC Client.
pub struct IpcClient;

impl IpcClient {
    /// Sends a single request to the named pipe and awaits the correlated response within `request.deadline_ms`.
    ///
    /// Resolves Finding IVA-P1-003: Enforces client-side timeout so the client never deadlocks on hanging servers.
    pub async fn send_request(
        pipe_name: &str,
        request: &IpcRequest,
    ) -> Result<IpcResponse, io::Error> {
        let deadline_duration = Duration::from_millis(request.deadline_ms.max(50));

        tokio::time::timeout(deadline_duration, async {
            let mut client = ClientOptions::new().open(pipe_name)?;

            let encoded = encode_request(request)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            client.write_all(&encoded).await?;
            client.flush().await?;

            let mut buffer = vec![0u8; 4096];
            let mut accumulated = Vec::new();

            loop {
                let n = client.read(&mut buffer).await?;
                if n == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "Server closed connection before delivering complete response",
                    ));
                }

                accumulated.extend_from_slice(&buffer[..n]);

                match decode_response(&accumulated) {
                    Ok((response, _)) => return Ok(response),
                    Err(FramingError::IncompleteFrame(..)) => continue,
                    Err(e) => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("Failed to decode response: {}", e),
                        ));
                    }
                }
            }
        })
        .await
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                format!("IPC request timed out after {}ms", request.deadline_ms),
            )
        })?
    }

    /// Convenience helper to send a command with JSON payload.
    pub async fn send_command(
        pipe_name: &str,
        command: impl Into<String>,
        payload: serde_json::Value,
    ) -> Result<IpcResponse, io::Error> {
        let request = IpcRequest::new(command, payload);
        Self::send_request(pipe_name, &request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::IpcServer;
    use std::sync::Arc;
    use tokio::sync::watch;

    #[tokio::test]
    async fn test_ipc_client_server_integration() {
        let pipe_name = format!(r"\\.\pipe\aethershell-test-{}", uuid::Uuid::new_v4());
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let handler = Arc::new(|req: IpcRequest| {
            Box::pin(async move {
                if req.command == "ping" {
                    IpcResponse::ok(req.message_id, serde_json::json!({ "pong": true }))
                } else {
                    IpcResponse::error(req.message_id, "Unknown command")
                }
            })
                as std::pin::Pin<Box<dyn std::future::Future<Output = IpcResponse> + Send>>
        });

        let server = IpcServer::new(&pipe_name, handler, shutdown_rx);
        let server_handle = tokio::spawn(async move {
            let _ = server.run().await;
        });

        // Give the pipe server a brief moment to create the instance
        tokio::time::sleep(Duration::from_millis(50)).await;

        let res = IpcClient::send_command(&pipe_name, "ping", serde_json::json!({}))
            .await
            .expect("send ping");

        assert!(res.success);
        assert_eq!(res.payload["pong"], true);

        // Shutdown server
        let _ = shutdown_tx.send(true);
        let _ = server_handle.await;
    }

    #[tokio::test]
    async fn test_client_timeout_enforcement_iva_p1_003() {
        let pipe_name = format!(
            r"\\.\pipe\aethershell-timeout-test-{}",
            uuid::Uuid::new_v4()
        );
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        // Handler that sleeps longer than the client's deadline
        let handler = Arc::new(|req: IpcRequest| {
            Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(500)).await;
                IpcResponse::ok(req.message_id, serde_json::json!({ "delayed": true }))
            })
                as std::pin::Pin<Box<dyn std::future::Future<Output = IpcResponse> + Send>>
        });

        let server = IpcServer::new(&pipe_name, handler, shutdown_rx);
        let server_handle = tokio::spawn(async move {
            let _ = server.run().await;
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        let mut req = IpcRequest::new("slow_op", serde_json::json!({}));
        req.deadline_ms = 100; // 100ms deadline vs 500ms server sleep

        let result = IpcClient::send_request(&pipe_name, &req).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);

        let _ = shutdown_tx.send(true);
        let _ = server_handle.await;
    }
}
