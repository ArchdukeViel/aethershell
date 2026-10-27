use aethershell_ipc::IpcClient;
use aethershell_script_host::{verify_script_digest, TrustError};
use mlua::Lua;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tracing::{error, info, warn};

fn json_to_lua<'lua>(lua: &'lua Lua, value: &serde_json::Value) -> mlua::Result<mlua::Value<'lua>> {
    match value {
        serde_json::Value::Null => Ok(mlua::Value::Nil),
        serde_json::Value::Bool(b) => Ok(mlua::Value::Boolean(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(mlua::Value::Integer(i))
            } else if let Some(f) = n.as_f64() {
                Ok(mlua::Value::Number(f))
            } else {
                Ok(mlua::Value::Nil)
            }
        }
        serde_json::Value::String(s) => lua.create_string(s).map(mlua::Value::String),
        serde_json::Value::Array(arr) => {
            let table = lua.create_table()?;
            for (idx, item) in arr.iter().enumerate() {
                table.set(idx + 1, json_to_lua(lua, item)?)?;
            }
            Ok(mlua::Value::Table(table))
        }
        serde_json::Value::Object(map) => {
            let table = lua.create_table()?;
            for (key, item) in map {
                table.set(key.as_str(), json_to_lua(lua, item)?)?;
            }
            Ok(mlua::Value::Table(table))
        }
    }
}

fn lua_to_json(val: &mlua::Value) -> mlua::Result<serde_json::Value> {
    match val {
        mlua::Value::Nil => Ok(serde_json::Value::Null),
        mlua::Value::Boolean(b) => Ok(serde_json::Value::Bool(*b)),
        mlua::Value::Integer(i) => Ok(serde_json::Value::Number((*i).into())),
        mlua::Value::Number(n) => {
            if let Some(num) = serde_json::Number::from_f64(*n) {
                Ok(serde_json::Value::Number(num))
            } else {
                Ok(serde_json::Value::Null)
            }
        }
        mlua::Value::String(s) => Ok(serde_json::Value::String(s.to_str()?.to_string())),
        mlua::Value::Table(t) => {
            let len = t.raw_len();
            if len > 0 {
                let mut arr = Vec::with_capacity(len);
                for i in 1..=len {
                    let v: mlua::Value = t.raw_get(i)?;
                    arr.push(lua_to_json(&v)?);
                }
                Ok(serde_json::Value::Array(arr))
            } else {
                let mut map = serde_json::Map::new();
                for pair in t.clone().pairs::<String, mlua::Value>() {
                    let (k, v) = pair?;
                    map.insert(k, lua_to_json(&v)?);
                }
                Ok(serde_json::Value::Object(map))
            }
        }
        _ => Ok(serde_json::Value::Null),
    }
}

/// Registers the host `aether` API bindings with Lua 5.4 exposing telemetry, audio spectrum, and outbound bindings.
pub fn register_host_bindings_with_outbound(
    lua: &Lua,
    cached_telemetry: Arc<RwLock<serde_json::Value>>,
    cached_spectrum: Arc<RwLock<serde_json::Value>>,
    outbound_bindings: Arc<RwLock<HashMap<String, serde_json::Value>>>,
) -> mlua::Result<()> {
    let aether = lua.create_table()?;

    let telem_ref = Arc::clone(&cached_telemetry);
    aether.set(
        "get_telemetry",
        lua.create_function(move |lua, ()| {
            let val = telem_ref.read().unwrap().clone();
            json_to_lua(lua, &val)
        })?,
    )?;

    let spec_ref = Arc::clone(&cached_spectrum);
    aether.set(
        "get_spectrum",
        lua.create_function(move |lua, ()| {
            let val = spec_ref.read().unwrap().clone();
            json_to_lua(lua, &val)
        })?,
    )?;

    let out_ref = Arc::clone(&outbound_bindings);
    aether.set(
        "set_binding",
        lua.create_function(move |_lua, (key, value): (String, mlua::Value)| {
            if let Ok(json_val) = lua_to_json(&value) {
                if let Ok(mut map) = out_ref.write() {
                    map.insert(key, json_val);
                }
            }
            Ok(())
        })?,
    )?;

    let out_ref2 = Arc::clone(&outbound_bindings);
    aether.set(
        "publish_bindings",
        lua.create_function(move |_lua, table: mlua::Table| {
            if let Ok(mut map) = out_ref2.write() {
                for (k, v) in table.pairs::<String, mlua::Value>().flatten() {
                    if let Ok(json_val) = lua_to_json(&v) {
                        map.insert(k, json_val);
                    }
                }
            }
            Ok(())
        })?,
    )?;

    lua.globals().set("aether", aether)?;
    Ok(())
}

pub fn register_host_bindings(
    lua: &Lua,
    cached_telemetry: Arc<RwLock<serde_json::Value>>,
    cached_spectrum: Arc<RwLock<serde_json::Value>>,
) -> mlua::Result<()> {
    let dummy_outbound = Arc::new(RwLock::new(HashMap::new()));
    register_host_bindings_with_outbound(lua, cached_telemetry, cached_spectrum, dummy_outbound)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let args: Vec<String> = env::args().collect();

    // Dedicated deterministic memory allocation test mode for Job Object quota verification
    if let Some(pos) = args.iter().position(|a| a == "--test-alloc-mib") {
        if pos + 1 < args.len() {
            let mib: usize = args[pos + 1].parse().unwrap_or(0);
            println!("WORKER_STARTED:{}", std::process::id());
            let page_size = 4096;
            let total_bytes = mib * 1024 * 1024;
            let num_pages = total_bytes / page_size;
            let mut buffer = Vec::with_capacity(total_bytes);
            for _ in 0..num_pages {
                buffer.extend_from_slice(&[42u8; 4096]);
            }
            println!("ALLOCATED_{}_MIB_SUCCESS", mib);
            std::process::exit(0);
        }
    }

    if args.contains(&"--version".to_string()) {
        println!("aethershell-script-host 1.0.0");
        return Ok(());
    }

    let mut pipe_name = String::new();
    let mut expected_digest = String::new();
    let mut script_path = String::new();
    let run_once = args.contains(&"--once".to_string());

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
    let script_bytes = if !script_path.is_empty() {
        let bytes = fs::read(&script_path).map_err(|e| {
            error!("Could not read script file '{}': {}", script_path, e);
            e
        })?;

        if let Err(e) = verify_script_digest(&bytes, &expected_digest) {
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
                TrustError::InvalidPath(p) => {
                    error!(
                        "Security violation: Script path '{}' is invalid or does not exist. Terminating.",
                        p
                    );
                }
                TrustError::ConflictingTrustRecords(p) => {
                    error!(
                        "Security violation: Conflicting trust records discovered for script '{}'. Terminating.",
                        p
                    );
                }
                TrustError::MalformedTrustEntry(p, reason) => {
                    error!(
                        "Security violation: Malformed trust entry for script '{}': {}. Terminating.",
                        p, reason
                    );
                }
            }
            std::process::exit(1);
        }

        info!("Script digest verified successfully: {}", expected_digest);
        Some(bytes)
    } else {
        None
    };

    // 2. Setup Telemetry & Spectrum Caches
    let cached_telemetry = Arc::new(RwLock::new(serde_json::json!({
        "cpu.usage_percent": 0.0,
        "metrics.cpu": 0.0,
        "memory.used_gib": 0.0,
        "metrics.ram": 0.0,
    })));

    let cached_spectrum = Arc::new(RwLock::new(serde_json::json!({
        "bands": vec![0.0f32; 32],
        "is_silent": true,
        "should_auto_hide": true,
    })));

    // 3. Connect to Core IPC if pipe specified
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

        // Fetch initial telemetry & spectrum snapshot
        if let Ok(t_resp) =
            IpcClient::send_command(&pipe_name, "get_telemetry", serde_json::json!({})).await
        {
            if t_resp.success {
                *cached_telemetry.write().unwrap() = t_resp.payload;
            }
        }
        if let Ok(s_resp) =
            IpcClient::send_command(&pipe_name, "get_spectrum", serde_json::json!({})).await
        {
            if s_resp.success {
                *cached_spectrum.write().unwrap() = s_resp.payload;
            }
        }
    }

    // 4. Initialize real unrestricted Lua 5.4 engine (PRD Section 13.2 / Q32:B)
    let lua = Lua::new();
    let outbound_bindings = Arc::new(RwLock::new(HashMap::new()));
    register_host_bindings_with_outbound(
        &lua,
        Arc::clone(&cached_telemetry),
        Arc::clone(&cached_spectrum),
        Arc::clone(&outbound_bindings),
    )?;

    // 5. Load and execute script if supplied
    if let Some(ref bytes) = script_bytes {
        let chunk = lua.load(bytes.as_slice()).set_name(&script_path);
        if let Err(e) = chunk.exec() {
            error!("Lua runtime script error during execution: {}", e);
            std::process::exit(1);
        }

        // Invoke initial lifecycle hook if defined
        if let Ok(init_func) = lua.globals().get::<_, mlua::Function>("init") {
            if let Err(e) = init_func.call::<_, ()>(()) {
                error!("Lua init() error: {}", e);
            }
        }
        if let Ok(on_init_func) = lua.globals().get::<_, mlua::Function>("on_init") {
            if let Err(e) = on_init_func.call::<_, ()>(()) {
                error!("Lua on_init() error: {}", e);
            }
        }
    }

    if run_once {
        info!("Script worker completed single run; exiting.");
        return Ok(());
    }

    // 6. Spawn background IPC telemetry updater and heartbeat watchdog loop
    let pipe_for_bg = pipe_name.clone();
    let telem_bg = Arc::clone(&cached_telemetry);
    let spec_bg = Arc::clone(&cached_spectrum);
    let outbound_bg = Arc::clone(&outbound_bindings);
    let tick_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let tick_counter_bg = Arc::clone(&tick_counter);
    let has_script = script_bytes.is_some();

    if !pipe_for_bg.is_empty() {
        tokio::spawn(async move {
            let mut last_tick = 0u64;
            let mut no_progress_count = 0u32;

            loop {
                tokio::time::sleep(Duration::from_millis(1000)).await;

                let current_tick = tick_counter_bg.load(std::sync::atomic::Ordering::Relaxed);
                if has_script && current_tick == last_tick {
                    no_progress_count += 1;
                    if no_progress_count >= 2 {
                        warn!("Lua execution tick stalled; suppressing heartbeat to allow supervisor watchdog enforcement.");
                        continue;
                    }
                } else {
                    last_tick = current_tick;
                    no_progress_count = 0;
                }

                // Send watchdog heartbeat to supervisor
                let _ = IpcClient::send_command(
                    &pipe_for_bg,
                    "script_worker_heartbeat",
                    serde_json::json!({}),
                )
                .await;

                // Flush outbound custom bindings to Core IPC if any
                let pending: Option<HashMap<String, serde_json::Value>> = {
                    if let Ok(mut map) = outbound_bg.write() {
                        if !map.is_empty() {
                            Some(map.drain().collect())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                };
                if let Some(bindings) = pending {
                    let _ = IpcClient::send_command(
                        &pipe_for_bg,
                        "publish_script_bindings",
                        serde_json::json!({ "bindings": bindings }),
                    )
                    .await;
                }

                // Refresh telemetry cache
                if let Ok(t_resp) =
                    IpcClient::send_command(&pipe_for_bg, "get_telemetry", serde_json::json!({}))
                        .await
                {
                    if t_resp.success {
                        if let Ok(mut g) = telem_bg.write() {
                            *g = t_resp.payload;
                        }
                    }
                }

                // Refresh spectrum cache
                if let Ok(s_resp) =
                    IpcClient::send_command(&pipe_for_bg, "get_spectrum", serde_json::json!({}))
                        .await
                {
                    if s_resp.success {
                        if let Ok(mut g) = spec_bg.write() {
                            *g = s_resp.payload;
                        }
                    }
                }
            }
        });
    }

    info!("Script worker loop initialized.");

    // 7. Event loop ticking Lua updates
    loop {
        tokio::time::sleep(Duration::from_millis(100)).await;

        if let Ok(update_func) = lua.globals().get::<_, mlua::Function>("update") {
            if let Err(e) = update_func.call::<_, ()>(()) {
                warn!("Lua update() error: {}", e);
            }
        } else if let Ok(on_update_func) = lua.globals().get::<_, mlua::Function>("on_update") {
            if let Err(e) = on_update_func.call::<_, ()>(()) {
                warn!("Lua on_update() error: {}", e);
            }
        }

        tick_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lua_54_interpreter_and_unrestricted_standard_libs() {
        let lua = Lua::new();

        // Standard string and math libraries must be fully accessible (PRD 13.2 / Q32:B)
        let res: i32 = lua
            .load("return math.floor(math.sqrt(144))")
            .eval()
            .expect("math eval");
        assert_eq!(res, 12);

        let str_res: String = lua
            .load("return string.upper('aethershell')")
            .eval()
            .expect("string eval");
        assert_eq!(str_res, "AETHERSHELL");

        // Table operations
        let table_res: i32 = lua
            .load(
                r#"
                local t = { 10, 20, 30 }
                table.insert(t, 40)
                return #t
            "#,
            )
            .eval()
            .expect("table eval");
        assert_eq!(table_res, 4);
    }

    #[test]
    fn test_aether_host_api_bindings_telemetry_and_spectrum() {
        let lua = Lua::new();
        let telemetry = Arc::new(RwLock::new(serde_json::json!({
            "metrics.cpu": 42.5,
            "metrics.ram": 8.2,
            "cpu.usage_percent": 42.5,
            "power.battery_percent": 95.0,
        })));
        let spectrum = Arc::new(RwLock::new(serde_json::json!({
            "bands": [0.1, 0.5, 0.8, 0.3],
            "is_silent": false,
            "should_auto_hide": false,
        })));

        register_host_bindings(&lua, telemetry, spectrum).expect("register host bindings");

        // Verify script accessing telemetry through aether.get_telemetry()
        let cpu: f64 = lua
            .load("local t = aether.get_telemetry(); return t['metrics.cpu']")
            .eval()
            .expect("eval get_telemetry");
        assert!((cpu - 42.5).abs() < f64::EPSILON);

        // Verify script accessing spectrum through aether.get_spectrum()
        let peak_band: f64 = lua
            .load("local s = aether.get_spectrum(); return s.bands[3]")
            .eval()
            .expect("eval get_spectrum");
        assert!((peak_band - 0.8).abs() < f64::EPSILON);

        let is_silent: bool = lua
            .load("local s = aether.get_spectrum(); return s.is_silent")
            .eval()
            .expect("eval is_silent");
        assert!(!is_silent);
    }

    #[test]
    fn test_aether_host_api_publish_and_set_binding() {
        let lua = Lua::new();
        let telemetry = Arc::new(RwLock::new(serde_json::json!({})));
        let spectrum = Arc::new(RwLock::new(serde_json::json!({})));
        let outbound = Arc::new(RwLock::new(HashMap::new()));

        register_host_bindings_with_outbound(&lua, telemetry, spectrum, Arc::clone(&outbound))
            .expect("register host bindings");

        // 1. Test aether.set_binding
        lua.load(r#"aether.set_binding("script.custom_stat", 42.5)"#)
            .exec()
            .expect("set_binding number");
        lua.load(r#"aether.set_binding("user.message", "hello from lua")"#)
            .exec()
            .expect("set_binding string");

        {
            let map = outbound.read().unwrap();
            assert_eq!(
                map.get("script.custom_stat").unwrap(),
                &serde_json::json!(42.5)
            );
            assert_eq!(
                map.get("user.message").unwrap(),
                &serde_json::json!("hello from lua")
            );
        }

        // 2. Test aether.publish_bindings with table
        lua.load(
            r#"
            aether.publish_bindings({
                ["script.battery_health"] = "optimal",
                ["script.fan_rpm"] = 2400,
                ["script.is_boosted"] = true
            })
        "#,
        )
        .exec()
        .expect("publish_bindings");

        {
            let map = outbound.read().unwrap();
            assert_eq!(
                map.get("script.battery_health").unwrap(),
                &serde_json::json!("optimal")
            );
            assert_eq!(map.get("script.fan_rpm").unwrap(), &serde_json::json!(2400));
            assert_eq!(
                map.get("script.is_boosted").unwrap(),
                &serde_json::json!(true)
            );
        }
    }
}
