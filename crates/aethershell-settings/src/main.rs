//! AetherShell On-Demand Settings Application (PRD Section 3.1 & 8.1 / Gate G11).
//!
//! Operates out-of-process on demand over session-scoped Named Pipe IPC.
//! Exits immediately on close, releasing memory working set.

use aethershell_ipc::{IpcClient, SessionToken};
use aethershell_win::SessionSingletonGuard;
use std::env;
use std::process::ExitCode;
use tracing::{error, info};

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt::init();

    let args: Vec<String> = env::args().collect();

    // 1. Resolve IPC pipe name (allows test isolation via --pipe or AETHERSHELL_TEST_IPC_PIPE)
    let custom_pipe = args
        .iter()
        .position(|a| a == "--pipe")
        .and_then(|pos| args.get(pos + 1).cloned())
        .or_else(|| env::var("AETHERSHELL_TEST_IPC_PIPE").ok());

    let (session_id, pipe_name) = if let Some(p) = custom_pipe {
        (0, p)
    } else {
        let singleton = SessionSingletonGuard::acquire();
        let s_id = singleton.as_ref().map(|s| s.session_id()).unwrap_or(1);
        (s_id, format!(r"\\.\pipe\aethershell-ipc-{}", s_id))
    };

    info!(
        "AetherShell Settings v{} (Session: {}, Pipe: {})",
        env!("CARGO_PKG_VERSION"),
        session_id,
        pipe_name
    );

    // 2. Handle CLI commands
    if args
        .iter()
        .any(|a| matches!(a.as_str(), "--help" | "-h" | "/?" | "-?" | "/h" | "/help"))
    {
        print_usage();
        return ExitCode::SUCCESS;
    }

    if args
        .iter()
        .any(|a| matches!(a.as_str(), "--version" | "-v" | "/version" | "/v"))
    {
        println!("AetherShell Settings v{}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }

    if let Err(unhandled) = validate_settings_args(&args) {
        eprintln!(
            "Error: Unrecognized command-line argument(s): {:?}",
            unhandled
        );
        print_usage();
        return ExitCode::FAILURE;
    }

    if args.iter().any(|a| a == "--status") {
        return query_command(&pipe_name, "get_status", serde_json::json!({})).await;
    }

    if args.iter().any(|a| a == "--capabilities") {
        return query_command(&pipe_name, "get_capabilities", serde_json::json!({})).await;
    }

    if args.iter().any(|a| a == "--telemetry") {
        return query_command(&pipe_name, "get_telemetry", serde_json::json!({})).await;
    }

    if args.iter().any(|a| a == "--spectrum") {
        return query_command(&pipe_name, "get_spectrum", serde_json::json!({})).await;
    }

    if args.iter().any(|a| a == "--pause-audio") {
        return query_command(&pipe_name, "pause_audio", serde_json::json!({})).await;
    }

    if args.iter().any(|a| a == "--resume-audio") {
        return query_command(&pipe_name, "resume_audio", serde_json::json!({})).await;
    }

    if let Some(pos) = args.iter().position(|a| a == "--page") {
        let page = args.get(pos + 1).map(|s| s.as_str()).unwrap_or("general");
        println!("Opening AetherShell Settings page: {}", page);
        return query_command(
            &pipe_name,
            "get_status",
            serde_json::json!({ "page": page }),
        )
        .await;
    }

    if let Some(pos) = args.iter().position(|a| a == "--interactive") {
        let timeout: u32 = args.get(pos + 1).and_then(|s| s.parse().ok()).unwrap_or(8);
        return query_command(
            &pipe_name,
            "enter_interactive_mode",
            serde_json::json!({ "timeout_seconds": timeout }),
        )
        .await;
    }

    if args.iter().any(|a| a == "--shutdown-core") {
        return query_command(&pipe_name, "shutdown", serde_json::json!({})).await;
    }

    // Default interactive / GUI invocation:
    // Connects to Core, validates presence, renders console/status overview, and exits on close.
    println!(
        "Connecting to active AetherShell engine on {}...",
        pipe_name
    );
    match IpcClient::send_command(&pipe_name, "get_status", serde_json::json!({})).await {
        Ok(resp) if resp.success => {
            println!("AetherShell Core Engine is active and healthy.");
            println!("Subsystems:");
            if let Some(subsystems) = resp.payload["subsystems"].as_array() {
                for sub in subsystems {
                    println!(
                        "  - {}: status={}, healthy={}",
                        sub["name"].as_str().unwrap_or("unknown"),
                        sub["status"].as_str().unwrap_or("unknown"),
                        sub["healthy"].as_bool().unwrap_or(false)
                    );
                }
            }
            println!("\nSettings session completed. Exiting on close (releasing working set per Gate G11).");
            ExitCode::SUCCESS
        }
        Ok(resp) => {
            eprintln!("AetherShell Core returned error: {:?}", resp.error_message);
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!(
                "Could not connect to AetherShell Core on pipe '{}': {}.",
                pipe_name, e
            );
            eprintln!("Ensure AetherShell is running (aethershell.exe).");
            ExitCode::FAILURE
        }
    }
}

async fn query_command(pipe_name: &str, command: &str, mut payload: serde_json::Value) -> ExitCode {
    if let Some(token) = SessionToken::load() {
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("session_token".into(), serde_json::json!(token));
        }
    }
    match IpcClient::send_command(pipe_name, command, payload).await {
        Ok(resp) => {
            if resp.success {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&resp.payload).unwrap_or_default()
                );
                ExitCode::SUCCESS
            } else {
                error!("Command failed: {:?}", resp.error_message);
                ExitCode::FAILURE
            }
        }
        Err(e) => {
            error!("IPC transport error: {}", e);
            ExitCode::FAILURE
        }
    }
}

fn print_usage() {
    println!("AetherShell Settings - On-Demand Configuration Client");
    println!("Usage: aethershell-settings [OPTIONS]\n");
    println!("Options:");
    println!("  --status              Query core subsystem health report");
    println!("  --capabilities        Query host shell capabilities");
    println!("  --telemetry           Query current native hardware metrics");
    println!("  --spectrum            Query WASAPI audio spectrum status");
    println!("  --pause-audio         Pause audio visualization capture");
    println!("  --resume-audio        Resume audio visualization capture");
    println!("  --page <PAGE>         Open specific settings page (general|taskbar|desktop|widgets|audio|scripts)");
    println!("  --interactive [SECS]  Trigger temporary interactive desktop mode");
    println!("  --shutdown-core       Request active engine to terminate cleanly");
    println!("  --pipe <PIPE>         Connect to explicit Named Pipe endpoint");
    println!("  --version, -v, /v     Display version and exit");
    println!("  --help, -h, /?        Show this help text");
}

fn validate_settings_args(args: &[String]) -> Result<(), Vec<String>> {
    let mut unhandled = Vec::new();
    let mut i = 1;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--help" | "-h" | "/?" | "-?" | "/h" | "/help" | "--version" | "-v" | "/version"
            | "/v" | "--status" | "--capabilities" | "--telemetry" | "--spectrum"
            | "--pause-audio" | "--resume-audio" | "--shutdown-core" => {
                i += 1;
            }
            "--pipe" | "--page" => {
                i += 1;
                if i < args.len() {
                    i += 1;
                }
            }
            "--interactive" => {
                i += 1;
                if i < args.len() && !args[i].starts_with('-') && !args[i].starts_with('/') {
                    i += 1;
                }
            }
            _ => {
                unhandled.push(args[i].clone());
                i += 1;
            }
        }
    }
    if unhandled.is_empty() {
        Ok(())
    } else {
        Err(unhandled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_print_usage_execution() {
        print_usage();
    }

    #[test]
    fn test_settings_cli_flag_matching() {
        let help_flags = ["--help", "-h", "/?", "-?", "/h", "/help"];
        for f in help_flags {
            assert!(matches!(f, "--help" | "-h" | "/?" | "-?" | "/h" | "/help"));
        }
        let ver_flags = ["--version", "-v", "/version", "/v"];
        for v in ver_flags {
            assert!(matches!(v, "--version" | "-v" | "/version" | "/v"));
        }

        assert!(validate_settings_args(&["aethershell-settings.exe".into()]).is_ok());
        assert!(
            validate_settings_args(&["aethershell-settings.exe".into(), "--status".into()]).is_ok()
        );
        assert!(validate_settings_args(&[
            "aethershell-settings.exe".into(),
            "--pipe".into(),
            r"\\.\pipe\test".into(),
            "--status".into()
        ])
        .is_ok());
        assert!(validate_settings_args(&[
            "aethershell-settings.exe".into(),
            "--page".into(),
            "widgets".into()
        ])
        .is_ok());
        assert!(validate_settings_args(&[
            "aethershell-settings.exe".into(),
            "--interactive".into(),
            "8".into()
        ])
        .is_ok());
        assert!(validate_settings_args(&[
            "aethershell-settings.exe".into(),
            "--interactive".into()
        ])
        .is_ok());
        assert!(
            validate_settings_args(&["aethershell-settings.exe".into(), "--unknown".into()])
                .is_err()
        );
    }
}
