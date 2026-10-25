use aethershell_ipc::IpcClient;
use aethershell_script_host::{verify_script_digest, TrustError};
use std::env;
use std::fs;
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let args: Vec<String> = env::args().collect();

    if args.contains(&"--version".to_string()) {
        println!("aethershell-script-host 1.0.0");
        return Ok(());
    }

    let mut pipe_name = String::new();
    let mut expected_digest = String::new();
    let mut script_path = String::new();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--pipe" if i + 1 < args.len() => {
                pipe_name = args[i + 1].clone();
                i += 1;
            }
            "--digest" if i + 1 < args.len() => {
                expected_digest = args[i + 1].clone();
                i += 1;
            }
            "--script" if i + 1 < args.len() => {
                script_path = args[i + 1].clone();
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }

    info!(
        "Starting AetherShell Script Host (PID: {})",
        std::process::id()
    );

    // 1. Verify Trust & Digest if script supplied (PRD Section 13.2 / Gate G10)
    if !script_path.is_empty() {
        let script_bytes = fs::read(&script_path).map_err(|e| {
            error!("Could not read script file '{}': {}", script_path, e);
            e
        })?;

        if let Err(e) = verify_script_digest(&script_bytes, &expected_digest) {
            match e {
                TrustError::DigestMismatch { expected, actual } => {
                    error!(
                        "Security violation: Script digest mismatch! Expected {}, got {}. Terminating.",
                        expected, actual
                    );
                }
                TrustError::Untrusted => {
                    error!("Security violation: Script package lacks explicit user trust grant. Terminating.");
                }
                TrustError::IoError(err) => {
                    error!(
                        "I/O error during script verification: {}. Terminating.",
                        err
                    );
                }
            }
            std::process::exit(1);
        }

        info!("Script digest verified successfully: {}", expected_digest);
    }

    // 2. Connect to Core IPC if pipe specified
    if !pipe_name.is_empty() {
        info!("Connecting to Core IPC pipe: {}", pipe_name);
        let resp = IpcClient::send_command(
            &pipe_name,
            "script_worker_ready",
            serde_json::json!({ "pid": std::process::id() }),
        )
        .await;

        match resp {
            Ok(r) => info!("Core handshake established: success={}", r.success),
            Err(e) => error!("Failed to handshake with Core IPC: {}", e),
        }
    }

    info!("Script worker loop initialized.");
    Ok(())
}
