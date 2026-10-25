use crate::framing::{decode_request, encode_response};
use crate::protocol::{IpcRequest, IpcResponse};
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tokio::sync::watch;
use tracing::{error, info, warn};

pub type RequestHandler =
    Arc<dyn Fn(IpcRequest) -> Pin<Box<dyn Future<Output = IpcResponse> + Send>> + Send + Sync>;

/// Asynchronous Named Pipe IPC Server scoped to the user session.
pub struct IpcServer {
    pipe_name: String,
    handler: RequestHandler,
    shutdown_rx: watch::Receiver<bool>,
}

impl IpcServer {
    pub fn new(
        pipe_name: impl Into<String>,
        handler: RequestHandler,
        shutdown_rx: watch::Receiver<bool>,
    ) -> Self {
        Self {
            pipe_name: pipe_name.into(),
            handler,
            shutdown_rx,
        }
    }

    /// Runs the IPC server loop until a shutdown signal is received.
    pub async fn run(mut self) -> io::Result<()> {
        info!("Starting IPC server on pipe: {}", self.pipe_name);

        let mut first = true;
        loop {
            // Check shutdown signal
            if *self.shutdown_rx.borrow() {
                info!("IPC server received shutdown signal; stopping accept loop.");
                break;
            }

            let server = ServerOptions::new()
                .first_pipe_instance(first)
                .create(&self.pipe_name)?;
            first = false;

            tokio::select! {
                res = server.connect() => {
                    match res {
                        Ok(()) => {
                            let handler = Arc::clone(&self.handler);
                            tokio::spawn(async move {
                                if let Err(e) = handle_connection(server, handler).await {
                                    warn!("IPC client connection ended with error: {}", e);
                                }
                            });
                        }
                        Err(e) => {
                            warn!("Named pipe connect error: {}", e);
                        }
                    }
                }
                _ = self.shutdown_rx.changed() => {
                    if *self.shutdown_rx.borrow() {
                        info!("IPC server stopping gracefully.");
                        break;
                    }
                }
            }
        }

        Ok(())
    }
}

async fn handle_connection(mut server: NamedPipeServer, handler: RequestHandler) -> io::Result<()> {
    let mut buffer = vec![0u8; 4096];
    let mut accumulated = Vec::new();

    loop {
        let n = server.read(&mut buffer).await?;
        if n == 0 {
            break; // Client closed connection
        }

        accumulated.extend_from_slice(&buffer[..n]);

        // Attempt to decode a complete framed request
        match decode_request(&accumulated) {
            Ok((request, consumed)) => {
                accumulated.drain(..consumed);

                // Dispatch to handler
                let response = handler(request).await;

                // Encode response
                match encode_response(&response) {
                    Ok(encoded) => {
                        server.write_all(&encoded).await?;
                        server.flush().await?;
                    }
                    Err(e) => {
                        error!("Failed to encode IPC response: {}", e);
                        let err_resp = IpcResponse::error(
                            response.correlation_id,
                            format!("Encoding error: {}", e),
                        );
                        if let Ok(err_encoded) = encode_response(&err_resp) {
                            let _ = server.write_all(&err_encoded).await;
                        }
                    }
                }
            }
            Err(crate::framing::FramingError::IncompleteFrame(..)) => {
                // Wait for more data
                continue;
            }
            Err(e) => {
                error!("IPC framing error from client: {}", e);
                let err_resp = IpcResponse::error("unknown", format!("Malformed frame: {}", e));
                if let Ok(err_encoded) = encode_response(&err_resp) {
                    let _ = server.write_all(&err_encoded).await;
                }
                break;
            }
        }
    }

    Ok(())
}
