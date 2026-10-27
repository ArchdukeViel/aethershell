# AetherShell Implementation & Verification Ledger

**Document Revision:** 0.1.0-dev (Reconciled Application Stability Baseline)  
**Last Updated:** 2026-10-27  
**Current Status:** Phase A Integrated — Transition to Application Stability & Truthful Acceptance Baseline

---

## Release Withdrawal & Governance Reconciliation Notice

> **AUTHORITATIVE NOTICE — OWNER DECISIONS Q42:A, Q43:A, Q44:A, Q45:B, Q46:B**
>
> 1. **Premature Release Withdrawal (Q42:A & Q45:B)**:  
>    On 2026-10-26, GitHub Release `v1.0.0` (ID: `423768197`, Node ID: `RE_kwDOVW-nKs4ZQjCF`) and remote Git tag `refs/tags/v1.0.0` pointing to commit `1da34dc14bfc16d162edc34af9e7cdbe6813b313` were **formally withdrawn and deleted** from GitHub. The release had been published prematurely based on in-memory mocks, stubs, unreviewed self-merges (PRs #7–#12), and failing CI pipelines.
>
> 2. **Version Reservation (Q46:B)**:  
>    Version tag `v1.0.0` is strictly **reserved for the future genuinely verified stable production release** after all applicable acceptance gates pass empirical qualification and owner sign-off. Active development proceeds under verified baseline version `0.1.0-dev`.
>
> 3. **Evidence Preservation & Git Immutability (Q45:B)**:  
>    Commit history on `main` (including commit `1da34dc14bfc16d162edc34af9e7cdbe6813b313`) is preserved without destructive force-pushes or history rewriting. Full metadata, asset digests, CI error logs, and audit records are permanently preserved in `local-test-artifacts/v1.0.0_release_preservation_record.json` and logged under [EVD-008](EVIDENCE.md#evd-008-premature-release-v100-withdrawal--governance-reconciliation-audit).
>
> 4. **Public Repository (Q43:A)**:  
>    The `ArchdukeViel/aethershell` repository remains strictly public. Public source code availability does not imply stable product readiness until formal release qualification.
>
> 5. **Mandatory Owner Approval for Merges (Q44:A)**:  
>    No pull request may be merged into `main` without explicit, documented owner approval or authorized session delegation. AI subagent reviews provide technical verification.
>
> 6. **Phase A Integration (PR #20)**:  
>    On 2026-10-27, PR #20 was merged into `main` at commit `10c4205e573a7d5b7a06ae83859dee8d2a30d2b6` with explicit owner approval. It established fail-closed Named Pipe DACLs, caller SID validation, capability token authorization, atomic config replacement, suspended-process Job Object memory quotas, C++/WinUI 3 Settings JSON response validation, and test isolation.

---

## Reconciled 20-Gate Acceptance Matrix (PRD Section 18.2)

Following the Phase A remediation and independent adversarial audit, the project acceptance matrix has been reconciled against the authoritative requirements in **PRD Section 18.2**. The previous claim that 20/20 gates were `VERIFIED` reflected component-level unit test passes rather than complete operating system integration. 

In accordance with owner guidance, gates are classified using the truthful vocabulary:
- **`VERIFIED`**: 4 Gates (G03, G16, G18, G19)
- **`PARTIALLY_VERIFIED`**: 14 Gates (G01, G02, G04, G06, G07, G08, G09, G10, G11, G12, G13, G14, G17, G20)
- **`NOT_APPLICABLE`**: 1 Gate (G05 — No experimental Explorer extension in stable v1)
- **`DEFERRED`**: 1 Gate (G15 — Five-channel public distribution deferred per owner instruction)
- **`NOT_IMPLEMENTED`**: 0 Gates
- **`BLOCKED`**: 0 Gates

| Gate | PRD Pass Condition | Current Status | Concrete Reason & Evidence References | Outstanding Acceptance Requirements |
|:---|:---|:---:|:---|:---|
| **G01 Core startup** | Non-elevated core without mandatory driver/service/Explorer injection, session-scoped singleton, safe shutdown | `PARTIALLY_VERIFIED` | Session-scoped singleton mutex (`Local\AetherShell_Singleton_{SessionId}`), dedicated tray message pump, desktop host window lifecycle, and clean process exit work. Verified in `lifecycle_ipc_test.rs` and EVD-001/EVD-012. However, Explorer restart recovery is missing (`TaskbarCreated` is not registered), and system shutdown/logoff session handling requires verification. | Register and handle `TaskbarCreated` message to recreate tray icon and re-discover shell handles on Explorer restart; test clean shutdown under `WM_ENDSESSION`. |
| **G02 Windows compatibility** | Required Windows 10/11 x64 matrix; honest fallback on unsupported effects | `PARTIALLY_VERIFIED` | OS build and UBR detection via registry is implemented and tested for Win10 19045+ through Win11 26H2. Direct2D 1.1 / DirectWrite runtime functions on target platforms (`crates/aethershell-win/src/version.rs`, EVD-002). However, execution has only been validated on GitHub CI runners and a single local Windows 11 host. Full multi-build OS matrix testing across differing GPU drivers has not been executed. | Execute automated matrix tests across clean VMs representing each target Windows build family (Win10 22H2, Win11 22H2/23H2/24H2/26H2) and GPU driver tier. |
| **G03 Shell isolation** | All stable binaries and packages contain no Explorer extension; no hooks into critical protected processes | `VERIFIED` | Workspace source tree, build outputs, and packaging contain zero DLL injection modules, API detours, or hooks into `explorer.exe`, `dwm.exe`, `csrss.exe`, or other system processes. The stable binary architecture is strictly out-of-process and non-invasive. Verified in `taskbar_desktop_integration_test.rs` and EVD-003/EVD-012. | Continuous CI checks ensuring no injection or detours dependencies are added. |
| **G04 Taskbar recovery** | External effects gated; native fallback; no repeated fighting or Explorer loop | `PARTIALLY_VERIFIED` | Non-invasive conflict detection (`detect_taskbar_conflicts`) yields cleanly to competing customizers (TranslucentTB, StartAllBack, RetroBar). Safe mode enforces `NativeNoopBackend`. `ExternalAppearanceBackend` gates on verified capabilities and falls back safely to no-op on Windows 11 XAML taskbars (`crates/aethershell-taskbar/src/coordinator.rs`, EVD-004). However, dynamic recovery when Explorer crashes/restarts is not yet wired. | Dynamic recovery upon `TaskbarCreated` notification; VM pixel capture proving clean fallback without visual stutter or loops. |
| **G05 Experimental qualification** | Any opt-in Explorer extension: disposable-VM tests, then dedicated recoverable hardware qualification before limited experimental distribution (not stable gate if absent) | `NOT_APPLICABLE` | No experimental Explorer extension, XAML bridge DLL, or in-process injection module is included in the codebase or built by default. Per PRD Section 18.2 note ("not stable gate if absent") and owner decisions Q02:C / Q35:B, this gate is N/A until an experimental Explorer extension is developed and qualified. | None for stable core. If an opt-in experimental bridge is created in the future, disposable VM and recoverable hardware qualification reports will be required. |
| **G06 Desktop layering** | Wallpaper below passive widgets below desktop icons on qualifying builds; otherwise visibly unsupported/disabled | `PARTIALLY_VERIFIED` | `DesktopHostWindow` discovers `SHELLDLL_DefView` / `WorkerW` / `Progman` and positions the overlay surface behind desktop icons (`WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_LAYERED`). Clicks pass through to desktop icons (`crates/aethershell-desktop/src/window.rs`, EVD-005). However, pressing `Win+D` (Show Desktop) causes unowned `WS_POPUP` windows to lose Z-order or minimize, and there is no active shell event listener re-asserting bottom stacking. | Handle `Win+D` stacking re-assertion via shell hooks / foreground event monitoring; verify layering across live wallpaper and desktop icon lasso selection in VM tests. |
| **G07 Temporary Interactive Mode** | Tray/hotkey opens bounded overlay, user action works; Escape/timeout/focus change restores passive icon input | `PARTIALLY_VERIFIED` | Dismissal triggers (Escape key, 8-second inactivity timeout, focus change, session lock) are implemented in `InteractionController`. Tray menu item "Unlock Layout" can trigger interactive mode (`crates/aethershell-desktop/src/interaction.rs`, EVD-005). However, no global hotkey (`RegisterHotKey`) is implemented, `desktop_host_wnd_proc` lacks mouse hit-testing (`WM_LBUTTONDOWN`) to interact with widget controls, and the mode toggles styles on the full-screen window rather than a bounded overlay. | Register configurable global hotkey; implement spatial pointer hit-testing for interactive widget controls; record VM focus/hit-test video demonstrating interactive control clicks and automatic return to passive icon selection. |
| **G08 Sensor honesty** | Missing counters collapse/show unknown; optional external providers; tested non-waking sampling | `PARTIALLY_VERIFIED` | Native Win32 telemetry providers exist for CPU (`GetSystemTimes`), RAM (`GlobalMemoryStatusEx`), storage (`GetDiskFreeSpaceExW`), battery (`GetSystemPowerStatus`), and network (`GetIfTable2`), with honest reporting of unsupported states (GPU is omitted honestly per PRD [F22]). However, in `engine.rs`, polled metrics are discarded during the runtime loop, and the desktop scene renders hardcoded dummy values (24.5% CPU, 6.8 GB RAM). | Wire live telemetry poll results into dynamic layout bindings dictionary and feed to renderer; verify non-waking sampling intervals on battery power. |
| **G09 Local adaptive audio** | Default-on local WASAPI, endpoint recovery, battery 15 FPS default, AC 30 FPS, configurable 60 FPS, no persistent samples | `PARTIALLY_VERIFIED` | Real WASAPI shared-loopback capture thread initializes `IAudioClient`/`IAudioCaptureClient`, runs Cooley-Tukey 512-point FFT, computes logarithmic frequency bands, and operates strictly in-memory without disk persistence (PRD [S08]). However: (1) `IMMNotificationClient` is not implemented, so switching default audio devices leaves capture attached to dead endpoints; (2) polling loop runs at 66 Hz regardless of battery mode or audio silence; (3) `canvas.rs` hardcodes dummy spectrum bands `[0.2, 0.4, ...]` because no presentation loop pushes live bands to the renderer. | Implement `IMMNotificationClient` for endpoint migration; adapt capture polling based on AC/battery status and audio silence; feed real spectrum bands to `D2D1CanvasRenderer` in a continuous render loop. |
| **G10 Lua worker reliability and trust** | Script host separate and lazy; explicit digest-bound consent; crash/hang detected without core-blocking; IPC invalid payload rejected | `PARTIALLY_VERIFIED` | Child worker process (`aethershell-script-host.exe`) integrates authentic Lua 5.4 (`mlua`), runs in a Windows Job Object with 128 MB memory limit, uses `CREATE_SUSPENDED` containment, enforces pure-Rust SHA-256 script digest trust verification before execution, and implements a crash quarantine circuit breaker. IPC server enforces peer authorization tokens (`crates/aethershell-script-host/src/supervisor.rs`, EVD-006, EVD-010). However: (1) worker is never auto-spawned by `AppEngine` during normal runtime; (2) zero `.lua` widget scripts exist in `assets/`; (3) no binding-mutation API (`aether.set_binding`) is exposed to Lua scripts to push computed state back to Core; (4) per owner directive, Lua is unrestricted trusted code and should not be sandboxed with arbitrary CPU caps. | Wire on-demand worker spawning to engine profile loader; expose `aether.set_binding` for state updates; bundle first-party Lua widgets; test crash/hang recovery in VM without blocking Core tray. |
| **G11 WinUI Settings independence** | C++/WinRT process on-demand, exits on close, survives lost IPC, core survives crash | `PARTIALLY_VERIFIED` | Native C++/WinRT MSBuild project compiles a genuine 64-bit executable (`aethershell-settings.exe`, 515 KB) with zero Rust runtime markers. Employs authenticated Named Pipe IPC client with strict WinRT JSON response parsing, framing limits (1 MB), and fail-closed test token overrides. Exits with 0 MB resident RAM on close (`Application::Current().Exit()`). Verified in MSBuild and CI run `40447657280`. However, all XAML controls (switches, sliders, combo boxes) are non-functional placeholders not wired to configuration fields; Settings does not deserialize `config.toml` or invoke configuration save/load IPC endpoints. | Wire WinUI 3 XAML controls to Core `get_config`/`set_config` IPC endpoints; implement configuration state synchronization and persistence; test Settings crash isolation from running Core. |
| **G12 Schema/backup** | All TOML samples parse; migrations reversible; installed and portable share intended AppData store with conflict protection | `PARTIALLY_VERIFIED` | TOML configuration schema parsing (`AppConfig`), JSON draft-07 schemas, atomic write replacement (`ReplaceFileW` / `MoveFileExW`), and corrupted file `.bak` rotation are fully implemented and unit-tested in `crates/aethershell-config`. Canonical configuration identity resolves to `%LOCALAPPDATA%\AetherShell` per Q38:B. However, configuration schema migrations (v1 -> v2 versioning) and concurrent instance file-locking conflict protection across multiple running processes are not yet fully implemented or tested. | Implement versioned reversible schema migrations; test concurrent instance file-collision handling and locking semantics in multi-instance VM tests. |
| **G13 Performance** | Real measured workloads meet ratified budgets on baseline matrix; no false exact-zero claims | `PARTIALLY_VERIFIED` | `crates/aethershell-diagnostics/src/benchmark.rs` implements active benchmarking of 512-sample PCM synthesis, FFT processing, layout evaluation, and true private working set measurement via Win32 `K32GetProcessMemoryInfo`. Confirms synthetic tests meet limits (<0.5% CPU / <40 MB idle; <2.0% CPU / <65 MB active). However, because the engine does not yet run a continuous active Direct2D presentation loop or live telemetry/audio update pipeline, whole-application runtime performance on actual hardware across five representative runs remains unmeasured. | Wire real-time presentation loop and measure genuine five-run traces on target hardware under idle and active media visualization workloads. |
| **G14 Accessibility** | Keyboard, Narrator/UIA, high contrast, scaling, reduced motion, localized truncation rules | `PARTIALLY_VERIFIED` | High-contrast themes and palette definitions exist in `aethershell-renderer`. WinUI 3 XAML controls inherit standard Windows UI Automation (UIA) and keyboard navigation primitives. However, desktop widgets are rendered via direct Win32 Direct2D without UIA provider implementation, and keyboard navigation across desktop widgets requires completion of Interactive Mode hit-testing. | Implement UIA accessibility providers or accessible alternative representation for desktop widgets; audit Narrator and keyboard navigation in WinUI 3 Settings. |
| **G15 Five-channel release** | MSI, MSIX, Microsoft Store approved listing, WinGet, self-contained portable fully qualified; no partial stable release (Q40:A) | `DEFERRED` | Packaging scripts (WiX v4 MSI, MSIX packaging layout, WinGet manifest generation, self-contained portable staging) exist in `packaging/`. However, in accordance with explicit owner decisions (Q40:A, Q42:A, Q46:B) and current owner instructions, public distribution, code signing, Store submission, and WinGet publication are **intentionally deferred** until application stability and functionality are achieved. Release v1.0.0 was formally withdrawn. | Code signing certificate procurement; clean-VM installation/upgrade/uninstall testing for all 5 channels; Microsoft Store submission and accepted listing; WinGet package validation. |
| **G16 License/privacy** | Apache-2.0 LICENSE and dependency notices; no unsolicited analytics or raw audio upload | `VERIFIED` | Apache-2.0 `LICENSE` and complete `NOTICE` are present. Cargo dependency tree contains 0 cloud telemetry SDKs or analytics exfiltration hooks. WASAPI loopback capture processes PCM samples strictly in volatile memory with zero disk persistence. Compiler path remapping (`--remap-path-prefix` and MSVC `/pathmap`) is verified to eliminate developer usernames and workstation paths from release binaries (`.cargo/config.toml`, EVD-009, EVD-010). | Maintain privacy invariants and license checks in CI. |
| **G17 Safe Mode and recovery** | Restart safe mode, widget/worker quarantine, Explorer cleanup, rollback, IPC errors, update recovery | `PARTIALLY_VERIFIED` | `--safe-mode` command line switch cleanly bypasses hooks and forces `NativeNoopBackend`. Lua worker crash quarantine transitions to `SupervisorState::Quarantined` after repeated failures. Configuration self-healing recovers from corrupted TOML via `.bak` rotation (`crates/aethershell-app/src/engine.rs`, `crates/aethershell-config/src/storage.rs`). However, dynamic Explorer crash recovery (re-registering upon Explorer restart) and update rollback mechanisms are not yet implemented. | Wire dynamic Explorer restart recovery (`TaskbarCreated`); test complete failure-injection scenarios in disposable VMs. |
| **G18 Strict no-go and evidence** | No unverified critical behavior represented as supported; no physical-host disruptive tests; all gates signed off | `VERIFIED` | Premature release v1.0.0 was formally withdrawn (Q42:A, Q45:B). The project strictly adheres to Q37:A safety constraints (disruptive shell tests restricted to disposable VMs; zero injection into host Explorer). All 20 gates are honestly classified with concrete empirical status, and PR #20 merge required explicit owner authorization (Q44:A) and passing CI. Verified in `docs/EVIDENCE.md` and commit history. | Maintain honest evidence ledger and sign-offs through subsequent stability PRs. |
| **G19 Coexistence** | Detected competing taskbar style controller yields; does not terminate or modify third-party process | `VERIFIED` | `detect_taskbar_conflicts()` inspects running processes for known customization tools (`TranslucentTB`, `StartAllBack`, `ExplorerPatcher`, `RetroBar`). If detected, `TaskbarCoordinator` deterministically yields and selects `NativeNoopBackend` per PRD Q21:A without terminating, injecting into, or modifying third-party processes (`crates/aethershell-taskbar/src/coordinator.rs`, EVD-009). | Extend coexistence detection matrix for emerging shell customization utilities. |
| **G20 Packaging identity** | Same user data semantics across MSI/MSIX/Store/portable, startup toggles, signed migrators and rollback | `PARTIALLY_VERIFIED` | Canonical data path resolution is implemented in `crates/aethershell-config/src/storage.rs` targeting `%LOCALAPPDATA%\AetherShell` across all deployment types, adhering strictly to PRD Q38:B. However, cross-channel data migration, startup toggle coordination across packaged MSIX and unpackaged portable instances, and migration rollback have not been tested in a clean-OS VM migration suite. | Execute clean-OS VM migration suite validating identical configuration access across MSI, MSIX, and Portable unpackaged executions. |

---

## Technical Audit Findings & Verified Remediation Summary

An adversarial audit conducted originally revealed 17 specific defects across the codebase. Across Phases 1–7 and Phase A (PR #20), these foundational issues were addressed:

1. **Workspace Package Version Mismatch**: Reconciled workspace versions to `0.1.0-dev`. Hardcoded version strings removed.
2. **Missing DirectX & DirectComposition Renderer**: Remediated in PR #15 (`crates/aethershell-renderer/src/d2d.rs`) with Direct2D 1.1 factory, render target creation, DirectWrite text layouts, and DirectComposition visual tree integration.
3. **No Desktop Window Creation**: Remediated in PR #15 (`crates/aethershell-desktop/src/window.rs`) with `DesktopHostWindow` creation behind desktop icons via `WorkerW`/`Progman`/`SHELLDLL_DefView`.
4. **Interactive Mode Stacking Violation**: Remediated in PR #15 (`crates/aethershell-desktop/src/interaction.rs`) by removing `WS_EX_TOPMOST` and implementing 4 auto-dismiss triggers (Escape, timeout, focus loss, session lock).
5. **No-Op Taskbar Backend**: Remediated with coordinator integration, conflict yielding, and safe fallback.
6. **Synthetic WASAPI Audio**: Remediated in PR #16 (`crates/aethershell-audio/src/wasapi.rs`) with real COM-initialized `IAudioClient`/`IAudioCaptureClient` loopback capture thread and 512-point FFT analysis.
7. **Missing Lua 5.4 Interpreter**: Remediated in PR #16 (`crates/aethershell-script-host`) with native `mlua` 5.4 runtime integration and `aether` host API bindings.
8. **In-Memory Script Supervisor**: Remediated in PR #16 (`crates/aethershell-script-host/src/supervisor.rs`) with Windows Job Object memory quotas (128 MB) and process lifecycle supervision.
9. **WinUI 3 Stub Substitution**: Remediated in PR #17 & PR #20 (`apps/aethershell-settings`) with complete 6-view C++/WinRT MSBuild solution, Named Pipe IPC client, and 0 MB resident RAM on close.
10. **Incomplete WiX MSI**: Staged in `packaging/wix` with `perMachine` installation, staging runtime DLLs, themes, skins, and schemas (distribution deferred).
11. **Invalid MSIX Packaging**: Staged in `packaging/msix` with authentic 32-bpp PNG assets, strict semantic validation, and test signing (distribution deferred).
12. **WinGet Scope & Checksum Mismatch**: Configured in `packaging/winget` with automated SHA-256 calculation script (distribution deferred).
13. **Store Preflight Only**: Documented in `packaging/store/StorePreflight.md` (submission deferred).
14. **Portable Storage PRD Q38:B Violation**: Remediated in PR #17 (`crates/aethershell-config/src/storage.rs`) with canonical `%LOCALAPPDATA%\AetherShell` resolution across all packaging modes.
15. **Insecure IPC Named Pipes**: Remediated in PR #16 & PR #20 (`crates/aethershell-ipc/src/server.rs`) with upfront Windows DACLs at creation, client SID verification, and peer PID authorization.
16. **Synthetic Performance Benchmark**: Remediated in PR #17 (`crates/aethershell-diagnostics/src/benchmark.rs`) with active 512-sample harmonic PCM synthesis, real-time FFT spectrum analysis, layout engine evaluation, and true private working set measurement.
17. **Committed Binaries in Git History & Developer Path Leaks**: Cleaned tracked binaries from layout; configured compiler path remapping in build tooling (`--remap-path-prefix` and MSVC `/pathmap`) eliminating developer usernames and local machine paths from compiled binaries.

---

## Historical Session-Token Investigation Record

During Phase A remediation testing, the security properties of the session token located at `%LOCALAPPDATA%\AetherShell\session_{id}.token` were investigated. 

To ensure strict evidentiary honesty:
- **Verified Current State**: The directory `%LOCALAPPDATA%\AetherShell` currently exists and contains **0 items**.
- **Historical Testing Record**: During initial automated tests in commit `53e5c3e`, a temporary dummy credential was written and subsequently cleaned up by `SessionTokenGuard::drop()`. However, because no prior host filesystem snapshot was recorded before that execution, the current absence of files does not conclusively establish whether any artifact existed prior to that invocation.
- **Permanent Test Isolation Guardrail**: Commit `6ff97e75bbebc2b621c86397b8eb89612038910c` (merged in PR #20) implemented `TestIsolationGuard` rooted in `%TEMP%`. The C++ Settings client `LoadSessionToken` fails closed immediately if an explicit test token override cannot be read, never falling back to `%LOCALAPPDATA%`. Normal tests are strictly prohibited from touching `%LOCALAPPDATA%\AetherShell`.

---

## Authoritative Lua Architecture Preservation

Per **PRD Section 13.2** and owner decision **Q32:B**:
- **Unrestricted Trusted Code**: Lua 5.4 executes only in a distinct, explicitly trusted, on-demand child worker process (`aethershell-script-host.exe`). The worker process is not designed as a hostile-code security sandbox; rather, it isolates accidental faults from the core tray process.
- **No Arbitrary CPU Hard Cap**: In accordance with owner directives, no 20% hard CPU cap is assumed or imposed. Resource containment relies on Windows Job Object memory quotas (128 MB per PRD Table 17-1), pure-Rust SHA-256 pre-execution digest verification, process heartbeat monitoring, and crash quarantine circuit breaking.
- **On-Demand Lifecycle**: The child worker launches only when a trusted Lua script is active, and terminates when no scripted widgets remain.

---

## Revised Application-Stability Engineering Roadmap

With Phase A merged, engineering focus transitions entirely to **making AetherShell a genuinely stable, functional, and reliable Windows desktop application before resuming public distribution work**.

The proposed sequence of focused, independently reviewable pull requests is:

1. **PR #21: Acceptance Ledger & Evidence Reconciliation** *(Current)*
   - Update `docs/STATUS.md`, `docs/EVIDENCE.md`, and `README.md` with verified PR #20 merge evidence, truthful gate statuses, and the session-token historical record.
2. **PR #22: Explorer Restart Recovery & Notification Tray Restoration**
   - Register dynamic Windows message `TaskbarCreated` in `tray.rs` and `DesktopHostWindow`.
   - Re-register tray icon via `Shell_NotifyIconW(NIM_ADD, ...)` and re-probe desktop hierarchy (`WorkerW`/`Progman`) upon Explorer restart.
3. **PR #23: Real-Time Telemetry & WASAPI Spectrum Presentation Loop**
   - Wire an adaptive presentation timer loop in `AppEngine` (15/30/60 FPS based on power/audio state).
   - Feed live `telemetry.poll_all()` and `audio.current_bands()` into layout bindings, replacing hardcoded dummy values.
4. **PR #24: Core Configuration IPC Endpoints (`get_config` / `set_config`)**
   - Wire `ConfigManager` into `AppEngine` to load `config.toml` at startup.
   - Implement authenticated IPC handlers with schema validation, transactional updates, and atomic persistence.
5. **PR #25: Functional WinUI 3 Settings Controls & State Persistence**
   - In `apps/aethershell-settings/MainWindow.xaml.cpp`, query `get_config` on load to populate all switches, sliders, and combo boxes.
   - Wire control event handlers to emit `set_config` updates over IPC and persist changes atomically.
6. **PR #26: Direct2D DXGI Device Loss & GPU Recovery**
   - Expand device loss detection to handle `DXGI_ERROR_DEVICE_REMOVED`, `DXGI_ERROR_DEVICE_RESET`, and `DXGI_ERROR_DEVICE_HUNG`.
   - Implement bounded recreation with exponential backoff, preventing infinite recursion on driver crashes.
7. **PR #27: `IMMNotificationClient` Audio Device Migration & Power-Aware Operation**
   - Implement `IMMNotificationClient` in Rust to seamlessly migrate capture when default playback device changes.
   - Dynamically adjust polling intervals based on AC/battery status and audio silence.
8. **PR #28: Interactive Mode Global Hotkey & Pointer Hit-Testing**
   - Register a configurable global hotkey via `RegisterHotKey` for Temporary Interactive Mode.
   - Implement mouse message handling (`WM_LBUTTONDOWN`) in `desktop_host_wnd_proc` to hit-test against interactive widget nodes.
9. **PR #29: `PerMonitorV2` DPI Awareness & Multi-Monitor Topology Handling**
   - Declare `PerMonitorV2` DPI awareness in `aethershell-app`.
   - Handle `WM_DPICHANGED` dynamically resizing Direct2D targets and updating layout scale.
   - Handle `WM_DISPLAYCHANGE` refreshing overlay windows across monitor attach/detach events.
10. **PR #30: Supervised On-Demand Lua Worker Integration & Real Widget Update Flow**
    - Wire child worker auto-spawn in `AppEngine` when scripted profiles are loaded.
    - Expose `aether.set_binding(key, value)` to Lua scripts to push computed state back to Core.
    - Bundle standard first-party Lua widgets under `assets/widgets/`.
11. **PR #31: Representative Whole-Application Performance & Stability Qualification**
    - Measure live running AetherShell processes under representative widget, audio, and scripting workloads.
    - Compare measured CPU, RAM, GPU, and battery drain against PRD Section 14 budgets.
