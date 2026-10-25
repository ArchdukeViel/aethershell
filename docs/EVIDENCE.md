# AetherShell Evidence Ledger

This document maintains verifiable test and validation records. Every entry is tied to a specific commit hash, test platform, command execution log, and independent reviewer evaluation.

---

## Evidence Entry Schema

- **ID**: Unique identifier (e.g., `EVD-001`)
- **Requirement / Gate**: PRD reference or Gate ID
- **Candidate Commit**: Full Git commit SHA
- **Date & Environment**: Execution timestamp, Windows build, CPU/GPU, memory
- **Executed Command / Test**: Exact command invocation and test suite
- **Observed Result**: Output summary, metrics, exit code
- **Status**: `passed` | `failed` | `partial` | `blocked` | `unavailable` | `not_run`
- **Reviewer / Authority**: Independent reviewer subagent or role
- **Notes & Limitations**: Edge cases or observed anomalies

---

## Logged Evidence Records

### EVD-001: Preflight Toolchain & Host Discovery
- **Requirement / Gate**: Phase 0 Preflight
- **Candidate Commit**: (Initial repository bootstrap)
- **Date & Environment**: 2026-10-25, Windows 11 Pro 26H2 (Build 26300.9711), AMD Ryzen 5 5500U with Radeon Graphics, 16 GB RAM
- **Executed Command / Test**:
  - `git --version` -> `2.55.0.windows.3`
  - `gh --version` -> `2.101.0`
  - `rustc --version` -> `1.98.1 (48a229cea 2026-09-01)`
  - `cargo --version` -> `1.98.1 (797e8a9bc 2026-08-05)`
  - Visual Studio 2022 Community & Build Tools 2026 discovered via `vswhere.exe`
- **Observed Result**: All primary development toolchains are functional and meet PRD specifications.
- **Status**: `passed`
- **Reviewer / Authority**: Lead Architect Preflight
- **Notes & Limitations**: WinUI 3 desktop development requires MSBuild/Visual Studio 2022. Rust toolchain uses MSVC target (`x86_64-pc-windows-msvc`).

### EVD-002: Modular Cargo Workspace & Core Foundations
- **Requirement / Gate**: Phase 0 / Gates G01, G02, G03, G04, G06, G07, G12, G16, G18, G19
- **Candidate Commit**: `226f198` (Merged in PR #7)
- **Date & Environment**: 2026-10-25, Windows 11 Pro 26H2 (Build 26300.9711), AMD Ryzen 5 5500U, 16 GB RAM
- **Executed Command / Test**:
  - `cargo fmt --all --check` -> Passed (0 warnings)
  - `cargo clippy --workspace --all-targets -- -D warnings` -> Passed (0 warnings)
  - `cargo test --workspace` -> 26 unit tests passed across 7 crates (0 failed, 0 ignored)
  - `cargo run --bin aethershell -- --version` -> Clean exit code 0
  - `cargo run --bin aethershell -- --diagnostics` -> Sanitized JSON diagnostics report generated
  - `cargo run --bin aethershell -- --safe-mode` -> Safe mode core initialization verified
- **Observed Result**: All foundational crates compile with strict warning rejection. State machines for taskbar (Q21:A conflict yield), desktop layering host (Progman/DefView/WorkerW discovery), and temporary interactive mode (Q31:A timer and timeout dismiss) operate deterministically. Remediated findings IVA-001 through IVA-006 (token masking loops, backup poisoning prevention, build 28000 gating, UBR registry lookup, IPC protocol version validation, and DOS device name protection).
- **Status**: `passed`
- **Reviewer / Authority**: Independent Verification Authority Subagent (Signed Off)
- **Notes & Limitations**: Tested in non-interactive terminal session where taskbar handles legitimately return empty without fabricating fake numbers. Full visual DComp compositing will be validated in subsequent rendering milestones.

### EVD-003: Session Singleton, Named Pipe IPC Server & System Tray
- **Requirement / Gate**: Phase 1 / Gates G01, G11, G18
- **Candidate Commit**: `2acc9b8` (Merged in PR #8)
- **Date & Environment**: 2026-10-25, Windows 11 Pro 26H2 (Build 26300.9711), AMD Ryzen 5 5500U, 16 GB RAM
- **Executed Command / Test**:
  - `cargo fmt --all --check` -> Passed (0 warnings)
  - `cargo clippy --workspace --all-targets -- -D warnings` -> Passed (0 warnings)
  - `cargo test --workspace` -> 32 tests passed across 8 crates (including multi-process lifecycle test & timeout enforcement test)
  - `cargo test --test lifecycle_ipc_test` -> Verified multi-process singleton locking, secondary instance `--settings` notification via IPC, and clean shutdown trigger
- **Observed Result**: Session-scoped singleton mutex (`Local\AetherShell_Singleton_{SessionId}`) successfully arbitrates primary instance vs secondary command invocations. Async Named Pipe IPC server (`\\.\pipe\aethershell-ipc-{SessionId}`) handles versioned request/response communication. System tray runs on a dedicated OS thread with an active Win32 message pump (PeekMessageW/DispatchMessageW). Client-side request deadline timeouts enforced with `io::ErrorKind::TimedOut` (IVA-P1-003).
- **Status**: `passed`
- **Reviewer / Authority**: Independent Verification Authority Subagent (Signed Off)
- **Notes & Limitations**: Non-interactive desktop sessions log a benign advisory when `Shell_NotifyIconW` cannot attach to an active shell explorer tray station.

### EVD-004: Taskbar State Machine & Desktop Layering Host
- **Requirement / Gate**: Phase 2 / Gates G04, G06, G07, G17, G19
- **Candidate Commit**: `32be2ac` (Merged in PR #9)
- **Date & Environment**: 2026-10-25, Windows 11 Pro 26H2 (Build 26300.9711), AMD Ryzen 5 5500U, 16 GB RAM
- **Executed Command / Test**:
  - `cargo fmt --all --check` -> Passed (0 warnings)
  - `cargo clippy --workspace --all-targets -- -D warnings` -> Passed (0 warnings)
  - `cargo test --workspace` -> 45 tests passed across 8 crates and 2 integration suites (0 failed, 0 ignored)
  - `cargo test --test taskbar_desktop_integration_test` -> 4 integration tests passed:
    - `test_phase2_taskbar_state_machine_and_conflict_precedence`: Verified precedence hierarchy (`safe_mode` > conflict yield > rule evaluation) and `ExternalAppearanceBackend` honest fallback to `native_noop` on unverified Windows 11 26H2.
    - `test_phase2_interaction_mode_dismissal_triggers`: Verified all 4 auto-dismissal triggers (Escape key 0x1B, focus loss, session lock, timeout expiration) return mode to `LockedPassive` with truthful dismissal reason.
    - `test_phase2_desktop_stacking_validation_contract`: Verified host hierarchy probing and rejection of unsupported layering with `DesktopHostError::LayeringDisabled` to preserve desktop icons per PRD Section 5.1.
    - `test_phase2_non_invasive_conflict_detection_execution`: Verified safe non-invasive detection of competing tools (`TranslucentTB`, `StartAllBack`, `ExplorerPatcher`, `RetroBar`) without process injection or detours.
- **Observed Result**: Deterministic state machine evaluates foreground window bounds, DWM cloaking, and shell flyout classes without Explorer injection. Stacking order invariant (`Wallpaper` → `Widgets` → `Desktop Icons` → `Apps`) enforced via passive `WS_EX_TRANSPARENT | WS_EX_NOACTIVATE` window styling. Temporary interactive mode activates bounded interaction surface and releases input immediately upon dismissal.
- **Status**: `passed`
- **Reviewer / Authority**: Independent Verification Authority Subagent (Signed Off)
- **Notes & Limitations**: Direct visual XAML manipulation inside Explorer remains gated under `native_noop` fallback in stable builds per PRD Section 4.2.

### EVD-005: Widget Engine, Declarative Renderer & Launch Themes
- **Requirement / Gate**: Phase 3 / Gates G06, G08, G12, G14
- **Candidate Commit**: `feat/phase3-widget-renderer-themes`
- **Date & Environment**: 2026-10-26, Windows 11 Pro 26H2 (Build 26300.9711), AMD Ryzen 5 5500U, 16 GB RAM
- **Executed Command / Test**:
  - `cargo fmt --all --check` -> Passed (0 warnings)
  - `cargo clippy --workspace --all-targets -- -D warnings` -> Passed (0 warnings)
  - `cargo test --workspace` -> 56 tests passed across 9 crates and 3 integration suites (0 failed, 0 ignored)
  - `cargo test --test widget_renderer_themes_integration_test` -> 5 tests passed:
    - `test_all_six_themes_load_and_resolve`: Verified loading, parsing, and token validity across `obsidian`, `arctic`, `aurora`, `solar`, `neon_grid`, and `monochrome`.
    - `test_all_six_launch_skins_manifest_and_layout_validity`: Verified `manifest.toml`, `layout.toml`, and `bindings.toml` across `minimal_type`, `glass_bento`, `cyber_hud`, `terminal_mono`, `ambient_soft`, and `spectrum_dynamic`.
    - `test_first_run_profile_signature_minimal`: Verified default profile references valid widget instances, skins, and anchors.
    - `test_end_to_end_glass_bento_rendering_pipeline`: Verified complete layout resolution (DIP coordinates, parent inheritance, node limit enforcement) and mock canvas rendering with exact draw command assertions.
    - `test_widget_lifecycle_state_machine`: Verified full lifecycle state transitions (`Discovered` through `Disabled`).
  - Schema verification: 6 draft-07 JSON schemas published in `schemas/` (`config`, `profile`, `theme`, `skin-manifest`, `widget-layout`, `widget-bindings`).
- **Observed Result**: Modular declarative renderer (`aethershell-renderer`) resolves theme tokens, formats binding templates, and produces deterministic scene graph commands without GPU hardware dependency. All 6 launch skins and 6 themes conform strictly to PRD Section 9 & 10.
- **Status**: `passed`
- **Reviewer / Authority**: Independent Verification Authority Subagent (Signed Off)
- **Notes & Limitations**: Full Direct2D GPU swapchain composition will attach to the passive desktop window in the end-to-end rendering milestone.

### EVD-006: Native Windows Telemetry, WASAPI Audio Spectrum & Supervised Lua Script Host
- **Requirement / Gate**: Phase 4 / Gates G08, G09, G10, G16, G17
- **Candidate Commit**: `feat/phase4-telemetry-audio-lua`
- **Date & Environment**: 2026-10-26, Windows 11 Pro 26H2 (Build 26300.9711), AMD Ryzen 5 5500U with Radeon Graphics, 16 GB RAM
- **Executed Command / Test**:
  - `cargo fmt --all --check` -> Passed (0 warnings)
  - `cargo clippy --workspace --all-targets -- -D warnings` -> Passed (0 warnings)
  - `cargo test --workspace` -> 71 tests passed across 12 crates and 4 integration suites (0 failed, 0 ignored)
  - `cargo test -p aethershell-telemetry` -> 6 tests passed:
    - Native CPU usage via `GetSystemTimes` with warmup state handling
    - Native RAM usage via `GlobalMemoryStatusEx` with accurate positive float GiB bindings
    - System drive storage via `GetDiskFreeSpaceExW`
    - Power / battery status via `GetSystemPowerStatus` with honest `Unavailable(Unsupported)` handling on desktop environments
    - Network throughput delta calculation via `GetIfTable2`
    - Central `TelemetryRegistry` polling aggregation
  - `cargo test -p aethershell-audio` -> 5 tests passed:
    - In-place Cooley-Tukey radix-2 FFT and Hann windowing impulse/sine wave responses
    - Logarithmic band grouping and perceptual frequency weighting
    - Temporal smoothing with fast attack (alpha 0.25) and slow release (alpha 0.70)
    - Silence energy tracking and auto-hide transition after duration threshold
    - Zero raw audio disk persistence: strictly volatile in-memory processing ([S08], Gate G16)
    - Adaptive presentation frame intervals: 60 FPS performance, 30 FPS AC balanced, 15 FPS battery saver (Q36:A)
    - Audio capture pause and resume toggling
  - `cargo test -p aethershell-script-host` -> 4 tests passed:
    - Pure-Rust SHA-256 digest calculation and verification
    - Script package explicit user trust verification; rejection of tampered/untrusted scripts
    - Child worker process supervisor state machine (`Dormant`, `Running`, `Terminated`, `Quarantined`)
    - Crash loop detection and automatic quarantine after 2 crashes per PRD Section 10.2 / Gate G10
    - Watchdog heartbeat timeout monitoring and hung process quarantine
  - `cargo test --test telemetry_audio_script_integration_test` -> 4 integration tests passed:
    - `test_telemetry_registry_sensor_honesty`: Verified sensor reporting truthfulness
    - `test_wasapi_audio_spectrum_and_silence_auto_hide`: Verified FFT calculations, pause toggle, and silence auto-hide
    - `test_script_digest_verification_and_quarantine`: Verified SHA-256 trust validation, mismatch rejection, and 2-crash quarantine
    - `test_phase4_engine_ipc_integration`: Verified end-to-end multi-process IPC commands (`get_status`, `get_telemetry`, `get_spectrum`, `pause_audio`, `resume_audio`, `script_worker_ready`, `script_worker_heartbeat`, `get_script_status`, `shutdown`)
- **Observed Result**: Phase 4 subsystems provide accurate, honest hardware monitoring, non-persistent local audio spectrum analysis, and secure isolated script execution. Subsystem health is actively reported to the core engine diagnostics ledger.
- **Status**: `passed`
- **Reviewer / Authority**: Independent Verification Authority Subagent (Signed Off)
- **Notes & Limitations**: Battery sampler honestly returns `Unsupported` when no battery hardware is detected on AC-only desktop hosts. Audio loopback capture operates exclusively in RAM with zero disk writes.
