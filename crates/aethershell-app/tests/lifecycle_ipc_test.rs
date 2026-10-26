use aethershell_ipc::IpcClient;
use std::process::{Command, Stdio};
use std::time::Duration;

#[test]
fn test_singleton_and_ipc_second_launch() {
    let target_bin = env!("CARGO_BIN_EXE_aethershell");

    // 1. Launch primary instance in background
    let mut child = Command::new(target_bin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("launch primary instance");

    // Give server time to bind mutex and named pipe
    std::thread::sleep(Duration::from_millis(600));

    // 2. Launch second instance with --settings
    let output = Command::new(target_bin)
        .arg("--settings")
        .output()
        .expect("launch secondary instance with --settings");

    let stdout_sec = String::from_utf8_lossy(&output.stdout);
    println!("Secondary instance output: {}", stdout_sec);

    // 3. Connect to named pipe and send "shutdown"
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        // Query status first
        // Attempt connection (session ID 1 or current)
        for session_id in 0..=3 {
            let pipe = format!(r"\\.\pipe\aethershell-ipc-{}", session_id);
            if let Ok(res) =
                IpcClient::send_command(&pipe, "get_status", serde_json::json!({})).await
            {
                assert!(res.success);

                // Negative test: unauthenticated shutdown must fail closed
                let unauth_res = IpcClient::send_command(&pipe, "shutdown", serde_json::json!({}))
                    .await
                    .expect("IPC call");
                assert!(
                    !unauth_res.success,
                    "Unauthenticated shutdown must be rejected"
                );

                // Authenticated shutdown using genuine session token capability
                let token = aethershell_ipc::SessionToken::load()
                    .expect("Session token must be present on disk for active engine");
                let auth_res = IpcClient::send_command(
                    &pipe,
                    "shutdown",
                    serde_json::json!({ "session_token": token }),
                )
                .await
                .expect("IPC call");
                assert!(auth_res.success, "Authenticated shutdown must succeed");
                break;
            }
        }
    });

    // Await primary exit
    let exit_status = child.wait().expect("child exit");
    assert!(exit_status.success());
}
