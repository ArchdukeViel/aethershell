//! Gate G03 Shell Isolation & Protected Process Non-Interference Integration Test
//!
//! Authoritative Requirement (PRD Section 18.2 Gate G03, Section 0 Principle 5, Section 13):
//! "All stable binaries and packages contain no Explorer extension; no hooks into critical protected processes."
//!
//! Verifies:
//! 1. All compiled binaries are native 64-bit AMD64 PE binaries with zero COM in-process server exports.
//! 2. Zero detour libraries (detours.dll, minhook.x64.dll, etc.) in import tables.
//! 3. Zero cross-process thread injection APIs (CreateRemoteThread, NtCreateThreadEx, SetWindowsHookEx) in import tables.
//! 4. Zero prohibited Explorer shell extension interfaces (IShellIconOverlayIdentifier, IDeskBand, IBrowserHelperObject).
//! 5. Runtime non-interference: running Explorer process contains strictly zero AetherShell modules.

use std::fs;
use std::path::{Path, PathBuf};

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
                    u32::from_le_bytes(bytes[exp_off + 24..exp_off + 28].try_into().unwrap())
                        as usize;
                let names_rva =
                    u32::from_le_bytes(bytes[exp_off + 32..exp_off + 36].try_into().unwrap());
                if let Some(names_off) = rva_to_offset(names_rva) {
                    for i in 0..num_names.min(4096) {
                        if names_off + i * 4 + 4 <= bytes.len() {
                            let fn_rva = u32::from_le_bytes(
                                bytes[names_off + i * 4..names_off + i * 4 + 4]
                                    .try_into()
                                    .unwrap(),
                            );
                            if let Some(fn_off) = rva_to_offset(fn_rva) {
                                if let Some(name) = read_ascii(fn_off) {
                                    exports.push(name);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if let Some(mut imp_off) = rva_to_offset(import_rva) {
        while imp_off + 20 <= bytes.len() {
            let orig_first_thunk =
                u32::from_le_bytes(bytes[imp_off..imp_off + 4].try_into().unwrap());
            let name_rva =
                u32::from_le_bytes(bytes[imp_off + 12..imp_off + 16].try_into().unwrap());
            let first_thunk =
                u32::from_le_bytes(bytes[imp_off + 16..imp_off + 20].try_into().unwrap());
            if orig_first_thunk == 0 && name_rva == 0 && first_thunk == 0 {
                break;
            }
            if let Some(name_off) = rva_to_offset(name_rva) {
                if let Some(dll_name) = read_ascii(name_off) {
                    imports.push(dll_name);
                }
            }
            let thunk_rva = if orig_first_thunk != 0 {
                orig_first_thunk
            } else {
                first_thunk
            };
            if let Some(mut thunk_off) = rva_to_offset(thunk_rva) {
                while thunk_off + if is_64bit { 8 } else { 4 } <= bytes.len() {
                    if is_64bit {
                        let val =
                            u64::from_le_bytes(bytes[thunk_off..thunk_off + 8].try_into().unwrap());
                        if val == 0 {
                            break;
                        }
                        thunk_off += 8;
                        if (val & 0x8000000000000000) == 0 {
                            let hint_rva = (val & 0xFFFFFFFF) as u32;
                            if let Some(fn_off) = rva_to_offset(hint_rva) {
                                if let Some(fn_name) = read_ascii(fn_off + 2) {
                                    imported_apis.push(fn_name);
                                }
                            }
                        }
                    } else {
                        let val =
                            u32::from_le_bytes(bytes[thunk_off..thunk_off + 4].try_into().unwrap());
                        if val == 0 {
                            break;
                        }
                        thunk_off += 4;
                        if (val & 0x80000000) == 0 {
                            if let Some(fn_off) = rva_to_offset(val) {
                                if let Some(fn_name) = read_ascii(fn_off + 2) {
                                    imported_apis.push(fn_name);
                                }
                            }
                        }
                    }
                }
            }
            imp_off += 20;
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

fn locate_workspace_binaries() -> Vec<(String, PathBuf)> {
    let app_bin = PathBuf::from(env!("CARGO_BIN_EXE_aethershell"));
    let bin_dir = app_bin.parent().unwrap();

    let mut result = Vec::new();
    result.push(("aethershell.exe".to_string(), app_bin.clone()));

    let script_host = bin_dir.join("aethershell-script-host.exe");
    if script_host.exists() {
        result.push(("aethershell-script-host.exe".to_string(), script_host));
    }

    let settings = bin_dir.join("aethershell-settings.exe");
    if settings.exists() {
        result.push(("aethershell-settings.exe".to_string(), settings));
    }

    // Also check target/x86_64-pc-windows-msvc/release
    let release_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/x86_64-pc-windows-msvc/release");
    if release_dir.exists() {
        for name in &[
            "aethershell.exe",
            "aethershell-script-host.exe",
            "aethershell-settings.exe",
        ] {
            let p = release_dir.join(name);
            if p.exists() && !result.iter().any(|(n, _)| n == *name) {
                result.push((name.to_string(), p));
            }
        }
    }

    result
}

#[test]
fn test_shell_isolation_binary_architecture_and_headers_gate_g03() {
    let binaries = locate_workspace_binaries();
    assert!(
        !binaries.is_empty(),
        "Must locate at least the primary aethershell executable"
    );

    for (name, path) in binaries {
        let bytes = fs::read(&path)
            .unwrap_or_else(|e| panic!("Failed to read binary {} at {:?}: {}", name, path, e));
        let pe =
            parse_pe(&bytes).unwrap_or_else(|| panic!("Binary {} must be a valid PE file", name));

        assert_eq!(
            pe.machine, 0x8664,
            "Binary {} must be AMD64 (0x8664), got 0x{:04X}",
            name, pe.machine
        );
        assert!(
            pe.is_64bit,
            "Binary {} must be 64-bit PE32+ optional header",
            name
        );
    }
}

#[test]
fn test_shell_isolation_zero_com_inproc_exports_gate_g03() {
    let forbidden_exports = ["DllRegisterServer", "DllUnregisterServer", "DllInstall"];

    let binaries = locate_workspace_binaries();
    for (name, path) in binaries {
        let bytes = fs::read(&path).unwrap();
        let pe = parse_pe(&bytes).unwrap();

        for exp in &pe.exports {
            for forbidden in &forbidden_exports {
                assert!(
                    !exp.eq_ignore_ascii_case(forbidden),
                    "Gate G03 VIOLATION: Binary {} exports COM in-process server registration function '{}'",
                    name,
                    exp
                );
            }
        }
        // Core and script-host binaries must export zero functions
        if name == "aethershell.exe" || name == "aethershell-script-host.exe" {
            assert!(
                pe.exports.is_empty(),
                "Binary {} should export 0 functions, found: {:?}",
                name,
                pe.exports
            );
        }
        // Settings binary must strictly never export COM in-process server registration or class factory
        if name == "aethershell-settings.exe" {
            for exp in &pe.exports {
                assert!(
                    !exp.eq_ignore_ascii_case("DllGetClassObject")
                        && !exp.eq_ignore_ascii_case("DllRegisterServer")
                        && !exp.eq_ignore_ascii_case("DllUnregisterServer")
                        && !exp.eq_ignore_ascii_case("DllInstall"),
                    "Gate G03 VIOLATION: Binary {} exports prohibited COM server function '{}'",
                    name,
                    exp
                );
            }
        }
    }
}

#[test]
fn test_shell_isolation_zero_detour_and_injection_imports_gate_g03() {
    let forbidden_dlls = [
        "detours.dll",
        "minhook.dll",
        "minhook.x64.dll",
        "minhook.x86.dll",
        "easyhook.dll",
        "easyhook64.dll",
        "easyhook32.dll",
        "mhook.dll",
        "subhook.dll",
        "polyhook.dll",
        "plthook.dll",
    ];

    let forbidden_apis = [
        "CreateRemoteThread",
        "CreateRemoteThreadEx",
        "NtCreateThreadEx",
        "RtlCreateUserThread",
        "SetWindowsHookA",
        "SetWindowsHookW",
        "SetWindowsHookExA",
        "SetWindowsHookExW",
        "QueueUserAPC",
        "QueueUserAPC2",
        "NtQueueApcThread",
        "ZwQueueApcThread",
    ];

    let binaries = locate_workspace_binaries();
    for (name, path) in binaries {
        let bytes = fs::read(&path).unwrap();
        let pe = parse_pe(&bytes).unwrap();

        for imp_dll in &pe.imports {
            for forbidden in &forbidden_dlls {
                assert!(
                    !imp_dll.eq_ignore_ascii_case(forbidden),
                    "Gate G03 VIOLATION: Binary {} imports prohibited detour library '{}'",
                    name,
                    imp_dll
                );
            }
        }

        for imp_api in &pe.imported_apis {
            for forbidden in &forbidden_apis {
                assert!(
                    !imp_api.eq_ignore_ascii_case(forbidden),
                    "Gate G03 VIOLATION: Binary {} imports prohibited injection/hook API '{}'",
                    name,
                    imp_api
                );
            }
        }
    }
}

#[test]
fn test_shell_isolation_zero_explorer_extension_markers_gate_g03() {
    let forbidden_markers: [&[u8]; 10] = [
        b"IShellIconOverlayIdentifier",
        b"IContextMenu",
        b"IExplorerCommand",
        b"IShellExecuteHook",
        b"IShellFolder",
        b"IDeskBand",
        b"IDeskBand2",
        b"IBrowserHelperObject",
        b"{00021492-0000-0000-C000-000000000046}",
        b"{06407553-9630-484F-A586-30CA07135045}",
    ];

    let binaries = locate_workspace_binaries();
    for (name, path) in binaries {
        let bytes = fs::read(&path).unwrap();

        for marker in &forbidden_markers {
            let found = bytes.windows(marker.len()).any(|window| window == *marker);
            let marker_str = String::from_utf8_lossy(marker);
            assert!(
                !found,
                "Gate G03 VIOLATION: Binary {} contains prohibited Explorer extension marker '{}'",
                name, marker_str
            );
        }
    }
}

#[test]
fn test_protected_process_non_interference_gate_g03() {
    // Non-invasive check: query running Explorer and critical protected processes via Windows API
    // Ensure no AetherShell modules are loaded in Explorer or protected system processes (PRD Section 18.2 Gate G03)
    use windows::Win32::Foundation::{CloseHandle, HMODULE};
    use windows::Win32::System::ProcessStatus::{
        EnumProcessModules, EnumProcesses, GetModuleFileNameExW,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
    };

    let target_protected_processes = [
        "explorer.exe",
        "dwm.exe",
        "csrss.exe",
        "svchost.exe",
        "audiodg.exe",
        "msedge.exe",
        "chrome.exe",
    ];

    let mut pids = [0u32; 2048];
    let mut bytes_returned = 0u32;

    unsafe {
        let enum_ok = EnumProcesses(
            pids.as_mut_ptr(),
            (pids.len() * std::mem::size_of::<u32>()) as u32,
            &mut bytes_returned,
        );
        if enum_ok.is_err() {
            // If process enumeration is unavailable in test environment, skip gracefully
            return;
        }

        let num_procs = (bytes_returned as usize) / std::mem::size_of::<u32>();
        let mut audited_process_count = 0;

        for &pid in &pids[..num_procs] {
            if pid == 0 {
                continue;
            }

            if let Ok(handle) = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid)
            {
                let mut modules = [HMODULE::default(); 512];
                let mut mod_bytes = 0u32;
                if EnumProcessModules(
                    handle,
                    modules.as_mut_ptr(),
                    (modules.len() * std::mem::size_of::<HMODULE>()) as u32,
                    &mut mod_bytes,
                )
                .is_ok()
                {
                    let count = (mod_bytes as usize) / std::mem::size_of::<HMODULE>();
                    if count > 0 {
                        let mut name_buf = [0u16; 260];
                        let len = GetModuleFileNameExW(handle, modules[0], &mut name_buf);
                        if len > 0 {
                            let main_mod_name = String::from_utf16_lossy(&name_buf[..len as usize]);
                            let lower = main_mod_name.to_lowercase();
                            let is_protected = target_protected_processes
                                .iter()
                                .any(|target| lower.ends_with(target));

                            if is_protected {
                                audited_process_count += 1;
                                for &m_inner in &modules[..count] {
                                    let mut inner_buf = [0u16; 260];
                                    let ilen =
                                        GetModuleFileNameExW(handle, m_inner, &mut inner_buf);
                                    if ilen > 0 {
                                        let iname =
                                            String::from_utf16_lossy(&inner_buf[..ilen as usize]);
                                        assert!(
                                            !iname.to_lowercase().contains("aethershell"),
                                            "Gate G03 VIOLATION: AetherShell module loaded in protected process {}: {}",
                                            lower,
                                            iname
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
                let _ = CloseHandle(handle);
            }
        }
        assert!(
            audited_process_count > 0,
            "Must locate and inspect at least one running protected process (e.g. explorer.exe)"
        );
    }
}
