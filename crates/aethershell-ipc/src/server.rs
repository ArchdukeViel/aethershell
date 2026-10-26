use crate::framing::{decode_request, encode_response};
use crate::protocol::{IpcRequest, IpcResponse};
use std::future::Future;
use std::io;
use std::os::windows::io::AsRawHandle;
use std::pin::Pin;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tokio::sync::watch;
use tracing::{error, info, warn};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL, WIN32_ERROR};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SetSecurityInfo,
    SDDL_REVISION_1, SE_KERNEL_OBJECT,
};
use windows::Win32::Security::{
    GetSecurityDescriptorDacl, GetTokenInformation, TokenUser, DACL_SECURITY_INFORMATION,
    PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, TOKEN_QUERY, TOKEN_USER,
};
use windows::Win32::System::Pipes::GetNamedPipeClientProcessId;
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

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
                .write_dac(true)
                .first_pipe_instance(first)
                .create(&self.pipe_name)?;
            first = false;

            // Apply Named Pipe DACL restricting access to the Current User Token SID
            let raw_handle = server.as_raw_handle();
            if let Err(e) = apply_current_user_dacl_to_handle(HANDLE(raw_handle as *mut _)) {
                warn!(
                    "Failed to restrict DACL on named pipe '{}': {}",
                    self.pipe_name, e
                );
            }

            tokio::select! {
                res = server.connect() => {
                    match res {
                        Ok(()) => {
                            let pipe_raw_handle = server.as_raw_handle();
                            let mut client_pid = 0u32;
                            let valid_pid = unsafe {
                                GetNamedPipeClientProcessId(HANDLE(pipe_raw_handle as *mut _), &mut client_pid).is_ok()
                            };

                            if !valid_pid || client_pid == 0 {
                                warn!("IPC connection rejected: could not validate client PID.");
                                continue;
                            }

                            info!("IPC client connected (verified PID: {})", client_pid);
                            let handler = Arc::clone(&self.handler);
                            tokio::spawn(async move {
                                if let Err(e) = handle_connection(server, handler, client_pid).await {
                                    warn!("IPC client (PID {}) connection ended with error: {}", client_pid, e);
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

/// Applies a restricted Discretionary Access Control List (DACL) to the named pipe kernel object
/// ensuring only the current user token SID possesses access (preventing cross-user escalation).
pub fn apply_current_user_dacl_to_handle(pipe_handle: HANDLE) -> Result<(), String> {
    unsafe {
        let mut token_handle = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token_handle)
            .map_err(|e| format!("OpenProcessToken failed: {}", e))?;

        let mut return_length = 0u32;
        let _ = GetTokenInformation(token_handle, TokenUser, None, 0, &mut return_length);
        if return_length == 0 {
            let _ = CloseHandle(token_handle);
            return Err("Failed to query token user buffer length".into());
        }

        let mut buffer = vec![0u8; return_length as usize];
        let info_res = GetTokenInformation(
            token_handle,
            TokenUser,
            Some(buffer.as_mut_ptr() as *mut _),
            return_length,
            &mut return_length,
        );
        let _ = CloseHandle(token_handle);
        info_res.map_err(|e| format!("GetTokenInformation failed: {}", e))?;

        let token_user = &*(buffer.as_ptr() as *const TOKEN_USER);
        let mut string_sid_ptr = windows::core::PWSTR::null();
        ConvertSidToStringSidW(token_user.User.Sid, &mut string_sid_ptr)
            .map_err(|e| format!("ConvertSidToStringSidW failed: {}", e))?;

        let sid_str = match string_sid_ptr.to_string() {
            Ok(s) => s,
            Err(e) => {
                let _ = LocalFree(HLOCAL(string_sid_ptr.0 as *mut _));
                return Err(format!("SID to_string failed: {}", e));
            }
        };
        let _ = LocalFree(HLOCAL(string_sid_ptr.0 as *mut _));

        // SDDL: Protected DACL with Generic All granted exclusively to Current User SID
        let sddl = format!("D:P(A;;GA;;;{})", sid_str);
        let sddl_u16: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();

        let mut p_sd = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl_u16.as_ptr()),
            SDDL_REVISION_1,
            &mut p_sd,
            None,
        )
        .map_err(|e| {
            format!(
                "ConvertStringSecurityDescriptorToSecurityDescriptorW failed: {}",
                e
            )
        })?;

        let mut dacl_present = windows::Win32::Foundation::BOOL(0);
        let mut p_dacl = std::ptr::null_mut();
        let mut dacl_defaulted = windows::Win32::Foundation::BOOL(0);
        GetSecurityDescriptorDacl(p_sd, &mut dacl_present, &mut p_dacl, &mut dacl_defaulted)
            .map_err(|e| {
                let _ = LocalFree(HLOCAL(p_sd.0 as *mut _));
                format!("GetSecurityDescriptorDacl failed: {}", e)
            })?;

        let set_res = SetSecurityInfo(
            pipe_handle,
            SE_KERNEL_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(p_dacl),
            None,
        );
        let _ = LocalFree(HLOCAL(p_sd.0 as *mut _));
        if set_res != WIN32_ERROR(0) {
            return Err(format!("SetSecurityInfo error: {:?}", set_res));
        }

        Ok(())
    }
}

async fn handle_connection(
    mut server: NamedPipeServer,
    handler: RequestHandler,
    _client_pid: u32,
) -> io::Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_apply_current_user_dacl_on_pipe() {
        let pipe_name = r"\\.\pipe\aethershell-test-dacl-handle";
        let server = ServerOptions::new()
            .write_dac(true)
            .first_pipe_instance(true)
            .create(pipe_name)
            .expect("create test pipe");
        let raw_handle = server.as_raw_handle();
        let res = apply_current_user_dacl_to_handle(HANDLE(raw_handle as *mut _));
        assert!(
            res.is_ok(),
            "Applying current user DACL must succeed: {:?}",
            res
        );
    }
}
