//! Gate G05 Experimental Qualification & Safety Boundary Integration Test
//!
//! Authoritative Requirement (PRD Section 18.2 Gate G05, Section 4.2 Backend C, Decisions Q02:C, Q35:B, Q37:A, Q41:A):
//! "Any opt-in Explorer extension: disposable-VM tests, then dedicated recoverable hardware qualification
//!  before limited experimental distribution (not stable gate if absent)."
//!
//! Verifies:
//! 1. Stable build invariant: Backend C (`ExplorerXamlExperimentalBackend`) strictly fails closed in stable releases.
//! 2. Safety boundary Q37:A: Physical daily-use host execution is strictly blocked for disruptive operations.
//! 3. Explicit consent requirement: Missing user consent token fails closed.
//! 4. Crash-loop recovery requirement: Unverified crash recovery fails closed.
//! 5. Safe Mode precedence: Safe Mode unconditionally suppresses any experimental backend, enforcing NativeNoop.
//! 6. Coordinator fallback: Any experimental error gracefully falls back to NativeNoop without loops or crashes.
//! 7. CLI flag rejection: `aethershell.exe --experimental-explorer-bridge` is strictly rejected with exit code 1.
//! 8. PE binary audit: Zero COM in-process exports (`DllGetClassObject`, `DllRegisterServer`), zero detour libraries,
//!    and zero cross-process thread injection APIs in release binaries.
//! 9. Host AppData isolation: `%LOCALAPPDATA%\AetherShell` remains strictly at 0 items.

use aethershell_core::{MonitorTaskbarState, TaskbarEffect};
use aethershell_taskbar::{
    ExplorerXamlExperimentalBackend, TaskbarBackend, TaskbarBackendError, TaskbarCoordinator,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn find_app_binary() -> Option<PathBuf> {
    let mut dir = std::env::current_exe().ok()?;
    dir.pop(); // Remove test exe name
    if dir.ends_with("deps") {
        dir.pop(); // Step out of deps
    }
    let candidate = dir.join("aethershell.exe");
    if candidate.exists() {
        return Some(candidate);
    }
    // Also check target/debug/aethershell.exe and target/release/aethershell.exe relative to workspace root
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent()?.parent()?;
    let debug_path = workspace
        .join("target")
        .join("x86_64-pc-windows-msvc")
        .join("debug")
        .join("aethershell.exe");
    if debug_path.exists() {
        return Some(debug_path);
    }
    let release_path = workspace
        .join("target")
        .join("x86_64-pc-windows-msvc")
        .join("release")
        .join("aethershell.exe");
    if release_path.exists() {
        return Some(release_path);
    }
    None
}

struct PeInfo {
    machine: u16,
    is_64bit: bool,
    exports: Vec<String>,
    imports: Vec<String>,
    imported_apis: Vec<String>,
}

fn parse_pe(bytes: &[u8]) -> Option<PeInfo> {
    if bytes.len() < 64 || bytes[0] != b'M' || bytes[1] != b'Z' {
        return None;
    }
    let pe_offset = u32::from_le_bytes(bytes[0x3C..0x40].try_into().ok()?) as usize;
    if pe_offset + 248 > bytes.len() {
        return None;
    }
    let sig = u32::from_le_bytes(bytes[pe_offset..pe_offset + 4].try_into().ok()?);
    if sig != 0x00004550 {
        return None;
    }
    let machine = u16::from_le_bytes(bytes[pe_offset + 4..pe_offset + 6].try_into().ok()?);
    let num_sections =
        u16::from_le_bytes(bytes[pe_offset + 6..pe_offset + 8].try_into().ok()?) as usize;
    let size_of_opt_header =
        u16::from_le_bytes(bytes[pe_offset + 20..pe_offset + 22].try_into().ok()?) as usize;
    let opt_header_offset = pe_offset + 24;
    let opt_magic = u16::from_le_bytes(
        bytes[opt_header_offset..opt_header_offset + 2]
            .try_into()
            .ok()?,
    );
    let is_64bit = opt_magic == 0x20B;

    let sections_offset = opt_header_offset + size_of_opt_header;
    struct Section {
        v_addr: u32,
        v_size: u32,
        raw_ptr: u32,
        raw_size: u32,
    }
    let mut sections = Vec::with_capacity(num_sections);
    for i in 0..num_sections {
        let s_off = sections_offset + i * 40;
        if s_off + 40 > bytes.len() {
            break;
        }
        let v_size = u32::from_le_bytes(bytes[s_off + 8..s_off + 12].try_into().unwrap());
        let v_addr = u32::from_le_bytes(bytes[s_off + 12..s_off + 16].try_into().unwrap());
        let raw_size = u32::from_le_bytes(bytes[s_off + 16..s_off + 20].try_into().unwrap());
        let raw_ptr = u32::from_le_bytes(bytes[s_off + 20..s_off + 24].try_into().unwrap());
        sections.push(Section {
            v_addr,
            v_size,
            raw_ptr,
            raw_size,
        });
    }

    let rva_to_offset = |rva: u32| -> Option<usize> {
        if rva == 0 {
            return None;
        }
        for s in &sections {
            let max_size = s.v_size.max(s.raw_size);
            if rva >= s.v_addr && rva < s.v_addr + max_size {
                return Some(((rva - s.v_addr) + s.raw_ptr) as usize);
            }
        }
        None
    };

    let read_ascii = |offset: usize| -> Option<String> {
        if offset >= bytes.len() {
            return None;
        }
        let mut end = offset;
        while end < bytes.len() && bytes[end] != 0 {
            end += 1;
        }
        std::str::from_utf8(&bytes[offset..end])
            .ok()
            .map(|s| s.to_string())
    };

    let mut exports = Vec::new();
    let mut imports = Vec::new();
    let mut imported_apis = Vec::new();

    let (export_rva, export_size, import_rva) = if is_64bit {
        if opt_header_offset + 128 <= bytes.len() {
            let exp = u32::from_le_bytes(
                bytes[opt_header_offset + 112..opt_header_offset + 116]
                    .try_into()
                    .unwrap(),
            );
            let exp_sz = u32::from_le_bytes(
                bytes[opt_header_offset + 116..opt_header_offset + 120]
                    .try_into()
                    .unwrap(),
            );
            let imp = u32::from_le_bytes(
                bytes[opt_header_offset + 120..opt_header_offset + 124]
                    .try_into()
                    .unwrap(),
            );
            (exp, exp_sz, imp)
        } else {
            (0, 0, 0)
        }
    } else if opt_header_offset + 112 <= bytes.len() {
        let exp = u32::from_le_bytes(
            bytes[opt_header_offset + 96..opt_header_offset + 100]
                .try_into()
                .unwrap(),
        );
        let exp_sz = u32::from_le_bytes(
            bytes[opt_header_offset + 100..opt_header_offset + 104]
                .try_into()
                .unwrap(),
        );
        let imp = u32::from_le_bytes(
            bytes[opt_header_offset + 104..opt_header_offset + 108]
                .try_into()
                .unwrap(),
        );
        (exp, exp_sz, imp)
    } else {
        (0, 0, 0)
    };

    if export_rva != 0 && export_size > 0 {
        if let Some(exp_off) = rva_to_offset(export_rva) {
            if exp_off + 40 <= bytes.len() {
                let num_names =
                    u32::from_le_bytes(bytes[exp_off + 24..exp_off + 28].try_into().unwrap());
                let names_rva =
                    u32::from_le_bytes(bytes[exp_off + 32..exp_off + 36].try_into().unwrap());
                if let Some(names_off) = rva_to_offset(names_rva) {
                    for i in 0..num_names {
                        let n_ptr_off = names_off + (i as usize) * 4;
                        if n_ptr_off + 4 <= bytes.len() {
                            let n_rva = u32::from_le_bytes(
                                bytes[n_ptr_off..n_ptr_off + 4].try_into().unwrap(),
                            );
                            if let Some(name_off) = rva_to_offset(n_rva) {
                                if let Some(name) = read_ascii(name_off) {
                                    exports.push(name);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if import_rva != 0 {
        if let Some(imp_off) = rva_to_offset(import_rva) {
            let mut curr = imp_off;
            while curr + 20 <= bytes.len() {
                let ilt_rva = u32::from_le_bytes(bytes[curr..curr + 4].try_into().unwrap());
                let name_rva = u32::from_le_bytes(bytes[curr + 12..curr + 16].try_into().unwrap());
                let iat_rva = u32::from_le_bytes(bytes[curr + 16..curr + 20].try_into().unwrap());
                if ilt_rva == 0 && name_rva == 0 && iat_rva == 0 {
                    break;
                }
                if let Some(dll_name_off) = rva_to_offset(name_rva) {
                    if let Some(dll_name) = read_ascii(dll_name_off) {
                        imports.push(dll_name);
                    }
                }

                let lookup_rva = if ilt_rva != 0 { ilt_rva } else { iat_rva };
                if let Some(lookup_off) = rva_to_offset(lookup_rva) {
                    let entry_size = if is_64bit { 8 } else { 4 };
                    let mut entry_curr = lookup_off;
                    while entry_curr + entry_size <= bytes.len() {
                        let is_ordinal = if is_64bit {
                            let val = u64::from_le_bytes(
                                bytes[entry_curr..entry_curr + 8].try_into().unwrap(),
                            );
                            if val == 0 {
                                break;
                            }
                            (val & 0x8000_0000_0000_0000) != 0
                        } else {
                            let val = u32::from_le_bytes(
                                bytes[entry_curr..entry_curr + 4].try_into().unwrap(),
                            );
                            if val == 0 {
                                break;
                            }
                            (val & 0x8000_0000) != 0
                        };

                        if !is_ordinal {
                            let hint_name_rva = if is_64bit {
                                (u64::from_le_bytes(
                                    bytes[entry_curr..entry_curr + 8].try_into().unwrap(),
                                ) & 0x7FFF_FFFF) as u32
                            } else {
                                u32::from_le_bytes(
                                    bytes[entry_curr..entry_curr + 4].try_into().unwrap(),
                                ) & 0x7FFF_FFFF
                            };

                            if let Some(hn_off) = rva_to_offset(hint_name_rva) {
                                if hn_off + 2 < bytes.len() {
                                    if let Some(api_name) = read_ascii(hn_off + 2) {
                                        imported_apis.push(api_name);
                                    }
                                }
                            }
                        }
                        entry_curr += entry_size;
                    }
                }
                curr += 20;
            }
        }
    }

    Some(PeInfo {
        machine,
        is_64bit,
        exports,
        imports,
        imported_apis,
    })
}

#[test]
fn test_experimental_backend_stable_build_fails_closed() {
    let mut backend = ExplorerXamlExperimentalBackend::new();
    assert!(backend.is_stable_build());

    let err = backend
        .apply_effect("mon-1", MonitorTaskbarState::Desktop, TaskbarEffect::Clear)
        .expect_err("Must fail closed in stable builds");

    assert_eq!(err, TaskbarBackendError::ExperimentalNotPermittedInStable);
}

#[test]
fn test_experimental_backend_physical_host_fails_closed() {
    // Simulated experimental build on physical host
    let mut backend = ExplorerXamlExperimentalBackend::with_params(
        false, // experimental build
        false, // physical host (not disposable VM)
        Some("valid-consent-token-2026".into()),
        true,
    );
    assert!(!backend.is_disposable_vm());

    let err = backend
        .apply_effect("mon-1", MonitorTaskbarState::Desktop, TaskbarEffect::Clear)
        .expect_err("Must fail closed on physical host");

    assert_eq!(err, TaskbarBackendError::PhysicalHostDisruptiveBlocked);
}

#[test]
fn test_experimental_backend_missing_consent_fails_closed() {
    // Simulated experimental build in disposable VM, but missing consent token
    let mut backend = ExplorerXamlExperimentalBackend::with_params(
        false, // experimental build
        true,  // disposable VM
        None,  // missing consent
        true,
    );
    assert!(!backend.has_user_consent());

    let err = backend
        .apply_effect("mon-1", MonitorTaskbarState::Desktop, TaskbarEffect::Clear)
        .expect_err("Must fail closed when consent is missing");

    assert_eq!(err, TaskbarBackendError::ExperimentalConsentMissing);
}

#[test]
fn test_experimental_backend_unverified_crash_recovery_fails_closed() {
    // Simulated experimental build in disposable VM with consent, but unverified crash recovery
    let mut backend = ExplorerXamlExperimentalBackend::with_params(
        false,
        true,
        Some("valid-consent-token-2026".into()),
        false, // unverified crash recovery
    );
    assert!(!backend.is_crash_recovery_verified());

    let err = backend
        .apply_effect("mon-1", MonitorTaskbarState::Desktop, TaskbarEffect::Clear)
        .expect_err("Must fail closed without crash recovery verification");

    match err {
        TaskbarBackendError::Unsupported(msg) => {
            assert!(msg.contains("Crash-loop recovery harness not qualified"));
        }
        other => panic!("Expected Unsupported error, got {:?}", other),
    }
}

#[test]
fn test_safe_mode_unconditionally_overrides_experimental_backend() {
    // Even if experimental backend was fully configured with VM and consent:
    let exp_backend = ExplorerXamlExperimentalBackend::with_params(
        false,
        true,
        Some("valid-consent-token-2026".into()),
        true,
    );

    let mut coordinator = TaskbarCoordinator::with_backend(Box::new(exp_backend));
    coordinator.set_safe_mode(true);

    let resolution = coordinator.evaluate_and_apply("mon-1", MonitorTaskbarState::Desktop);
    assert_eq!(resolution.applied_effect, TaskbarEffect::NativeNoop);
    assert_eq!(resolution.backend, "safe_mode");
    assert!(resolution
        .fallback_reason
        .as_ref()
        .unwrap()
        .contains("Safe mode active"));
}

#[test]
fn test_taskbar_coordinator_graceful_experimental_fallback_to_noop() {
    // Coordinator using default stable experimental backend fails closed gracefully
    let exp_backend = ExplorerXamlExperimentalBackend::new();
    let mut coordinator = TaskbarCoordinator::with_backend(Box::new(exp_backend));

    let resolution = coordinator.evaluate_and_apply("mon-1", MonitorTaskbarState::Desktop);
    assert_eq!(resolution.applied_effect, TaskbarEffect::NativeNoop);
    assert_eq!(resolution.backend, "native_noop");
    assert!(resolution
        .fallback_reason
        .as_ref()
        .unwrap()
        .contains("Experimental Explorer bridge is not permitted in stable v1 releases"));
}

#[test]
fn test_cli_rejects_experimental_explorer_bridge_flag() {
    let binary = match find_app_binary() {
        Some(b) => b,
        None => {
            eprintln!("aethershell.exe binary not yet compiled; skipping live execution test");
            return;
        }
    };

    let output = Command::new(&binary)
        .arg("--experimental-explorer-bridge")
        .output()
        .expect("Failed to execute aethershell.exe");

    assert!(
        !output.status.success(),
        "Expected non-zero exit code when passing --experimental-explorer-bridge"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("strictly prohibited in stable v1 core releases"),
        "Expected rejection notice in stderr, got: {}",
        stderr
    );
    assert!(
        stderr.contains("Gate G05 is NOT_APPLICABLE for stable v1.0.0"),
        "Expected G05 notice in stderr, got: {}",
        stderr
    );
}

#[test]
fn test_pe_exports_and_imports_zero_explorer_extensions() {
    let binary = match find_app_binary() {
        Some(b) => b,
        None => {
            eprintln!("aethershell.exe binary not yet compiled; skipping PE audit");
            return;
        }
    };

    let bytes = fs::read(&binary).expect("Failed to read aethershell.exe binary");
    let pe = parse_pe(&bytes).expect("Failed to parse PE headers of aethershell.exe");

    // 1. AMD64 Machine
    assert_eq!(pe.machine, 0x8664, "Binary must be native 64-bit AMD64");
    assert!(pe.is_64bit);

    // 2. Prohibited COM in-process server exports
    let prohibited_exports = [
        "DllGetClassObject",
        "DllCanUnloadNow",
        "DllRegisterServer",
        "DllUnregisterServer",
        "DllInstall",
    ];
    for exp in &pe.exports {
        for prohibited in &prohibited_exports {
            assert_ne!(
                exp.to_lowercase(),
                prohibited.to_lowercase(),
                "CRITICAL VIOLATION: Stable binary exports in-process COM entrypoint '{}'",
                exp
            );
        }
    }

    // 3. Prohibited Detour / In-Process Hooking Libraries
    let prohibited_imports = [
        "detours.dll",
        "minhook.x64.dll",
        "minhook.dll",
        "easyhook64.dll",
        "easyhook.dll",
    ];
    for imp in &pe.imports {
        for prohibited in &prohibited_imports {
            assert!(
                !imp.to_lowercase().contains(prohibited),
                "CRITICAL VIOLATION: Stable binary imports detour library '{}'",
                imp
            );
        }
    }

    // 4. Prohibited Cross-Process Thread Injection APIs
    let prohibited_apis = [
        "CreateRemoteThread",
        "CreateRemoteThreadEx",
        "NtCreateThreadEx",
        "ZwCreateThreadEx",
        "RtlCreateUserThread",
    ];
    for api in &pe.imported_apis {
        for prohibited in &prohibited_apis {
            assert_ne!(
                api.to_lowercase(),
                prohibited.to_lowercase(),
                "CRITICAL VIOLATION: Stable binary imports cross-process thread injection API '{}'",
                api
            );
        }
    }
}

#[test]
fn test_appdata_remains_strictly_zero_items_during_experimental_tests() {
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let aethershell_dir = PathBuf::from(local_app_data).join("AetherShell");
        if aethershell_dir.exists() {
            let count = fs::read_dir(&aethershell_dir)
                .map(|entries| entries.count())
                .unwrap_or(0);
            assert_eq!(
                count, 0,
                "%LOCALAPPDATA%\\AetherShell must strictly contain 0 items, found {}",
                count
            );
        }
    }
}
