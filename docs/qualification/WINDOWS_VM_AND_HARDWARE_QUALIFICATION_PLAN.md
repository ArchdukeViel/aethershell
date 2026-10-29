# AetherShell Windows VM & Physical Hardware Qualification Plan

**Document Version:** 1.0.0-draft  
**Baseline Commit:** Post-PR #51 (`8531c07c91dec960fde597ae97df9145f5b1bf6b`)  
**Target Release Phase:** Pre-Production Qualification Phase (PRD Section 18.2 / Section 14)  
**Governance Authority:** Authoritative PRD & Owner Directives (Q01–Q46, Q37:A, Q40:A, Q42:A, Q44:A, Q46:B)

---

## 1. Executive Summary & Objective

AetherShell has achieved clean component-level unit and integration test verification across its 13 workspace crates (302 unique passing tests across 27 test executables + 65 passing PowerShell regression tests across 13 test suites = 367 total passing automated tests). However, under the strict evidentiary governance defined in **PRD Section 18.2** and **AGENTS.md**, **no gate can be certified as `VERIFIED` based solely on in-process tests or headless CI runners**.

The objective of this Qualification Plan is to establish a rigorous, repeatable, and non-destructive empirical test campaign to evaluate AetherShell across:
1. Isolated, disposable Windows Virtual Machines (VMs) for disruptive shell and fault-injection testing.
2. Dedicated physical test hardware representing distinct GPU driver tiers and multi-monitor configurations.
3. Ratified PRD Section 14.2 multi-process performance budgets with live hardware instrumentation.

Successful execution and documentation of this plan is a mandatory prerequisite before any gate may transition to `VERIFIED` and before `CORE_STABLE_CANDIDATE` or production release `v1.0.0` may be formally declared.

---

## 2. Platform & Hardware Test Matrices

### 2.1 Target Operating System Matrix (Disposable VMs)

All disruptive shell and fault-injection tests must execute in clean, disposable Hyper-V or Windows Sandbox environments. In accordance with safety constraint **Q37:A**, tests that kill or restart `explorer.exe` or simulate shell failures must **NEVER** run on physical daily-use host machines. Sequential execution (strictly one VM at a time) is mandatory to respect the host's 16 GB physical RAM envelope.

| VM Profile ID | Windows Edition | Build Family | Status / Policy | Primary Test Objective |
|:---|:---|:---|:---|:---|
| **VM-WIN10-22H2** | Windows 10 Pro x64 | Build 19045 | Required minimum compatibility tier | Legacy taskbar fallback, Direct2D 1.1 compatibility, PerMonitorV2 DPI awareness. Windows 10 servicing differences documented separately. |
| **VM-WIN11-22H2** | Windows 11 Pro x64 | Build 22621 | Required legacy Win11 tier | XAML taskbar non-invasive detection, Desktop layering behind `SHELLDLL_DefView`. |
| **VM-WIN11-23H2** | Windows 11 Enterprise x64 | Build 22631 | Required compatibility tier | General stability baseline, Named Pipe DACL validation, session token lifecycle. |
| **VM-WIN11-24H2** | Windows 11 Pro x64 | Build 26100 | Required compatibility tier | Modern Windows 11 kernel, Job Object containment, UTF-8 BOM-free storage. |
| **VM-WIN11-25H2** | Windows 11 Pro x64 | Build 26200 | Primary compatibility tier | Mid-generation shell changes, XAML taskbar styling hooks, DirectComposition visual tree stability. |
| **VM-WIN11-26H2** | Windows 11 Pro x64 | Build 26300 | Primary compatibility tier (GA Oct 2026) | Primary Windows 11 26H2 compatibility baseline; UBR detection, modern shell message tracking. Not merely Canary. |
| **VM-WIN11-26H1** | Windows 11 Pro x64 | Build 28000 | Special validation tier | Distinct hardware-focused branch validation where qualifying x64 environment exists. |

*Note: Windows 11 21H2 (Build 22000) is evaluated separately and not advertised until empirical tests exist. Windows ARM64 is strictly outside the approved stable-v1 target.*

### 2.2 Physical Hardware & GPU Driver Tiers

Performance budgeting, frame pacing, audio loopback capture, and GPU device-loss recovery require bare-metal execution on physical hardware platforms:

| Hardware Tier ID | Hardware Specification | GPU & Driver Architecture | Audio & Display Topology |
|:---|:---|:---|:---|
| **HW-TIER-INTEL** | Intel Core i5/i7 (11th–13th Gen), 16 GB RAM | Intel Iris Xe / UHD Graphics (WDDM 3.1, Driver 31.0+) | Realtek HD Audio (WASAPI shared mode), Single 1080p @ 60 Hz (100% DPI). |
| **HW-TIER-AMD** | AMD Ryzen 5/7 5000/7000 Series, 16 GB RAM *(Empirical baseline recorded under EVD-035: AMD Ryzen 5 5500U, 16 GB RAM, AMD Radeon Graphics, Windows 11 Pro 26H2 Build 26300; 5x60s formal benchmark ledger in `docs/qualification/ryzen5_5500u_benchmark_ledger.json`)* | AMD Radeon Graphics / RX 6000+ (Adrenalin 23.12+, WDDM 3.1) | Realtek / AMD High Definition Audio, Single 1080p @ 60 Hz (100% DPI) / Dual Monitor: 1440p @ 144 Hz + 1080p @ 60 Hz. |
| **HW-TIER-NVIDIA** | Intel/AMD CPU, 32 GB RAM | NVIDIA GeForce RTX 3060/4070 (Game Ready / Studio Driver 550+) | HDMI/DP Audio + USB Audio DAC, Triple Monitor: 4K @ 60 Hz (150% DPI) + Dual 1440p. |

### 2.3 Software Rasterizer & Diagnostic Fallback Tier

For headless CI environments, virtual machines lacking GPU passthrough, or driver crash recovery diagnostics, AetherShell supports Microsoft Windows Advanced Rasterization Platform (WARP):

| Fallback Tier ID | Environment Specification | Rasterizer & Driver Architecture | Test Role & Diagnostic Objective |
|:---|:---|:---|:---|
| **SW-TIER-WARP** | Reference Low-End Laptop / Disposable VM | Microsoft Basic Render Driver (WARP Direct3D 11.1 software rasterizer) | Fallback rendering validation, driver reset fallback simulation, battery power simulation without discrete GPU. |

---

## 3. Authoritative Acceptance Gate Verification Procedures (PRD Section 18.2)

Every gate from G01 through G20 is mapped directly to its canonical PRD requirement. Gates are verified individually with dedicated procedures, explicit pass criteria, and target test environments.

### Gate G01: Core Startup, Session Management & Clean Shutdown
- **Authoritative Requirement:** Non-elevated core without mandatory driver/service/Explorer injection; session-scoped singleton; safe startup and shutdown.
- **Target Environment:** VM-WIN11-24H2, VM-WIN10-22H2, HW-TIER-AMD (non-invasive observations only).
- **Test Procedure:**
  1. Launch `aethershell.exe` in standard user context (Medium Mandatory Level, non-elevated).
  2. Verify session-scoped singleton mutex `Local\AetherShell_Singleton_{SessionId}` prevents secondary instance execution and routes CLI arguments (`--settings`) to primary instance over Named Pipe.
  3. Initiate system logoff / restart while AetherShell is running.
  4. Verify window message handler intercepts `WM_QUERYENDSESSION` and `WM_ENDSESSION`, executing graceful shutdown within 1.5 seconds without blocking Windows shutdown or triggering `WerFault.exe`.
- **Pass Criteria:** Zero-exit code; clean cleanup of tray and desktop window handles; zero orphaned child processes (`aethershell-script-host.exe`).

### Gate G02: Windows Compatibility, Multi-Build Matrix & Dynamic DPI
- **Authoritative Requirement:** Required Windows 10/11 x64 matrix; honest fallback on unsupported effects; dynamic PerMonitorV2 DPI awareness.
- **Target Environment:** Clean disposable VMs across all 7 target tiers (VM-WIN10-22H2 through VM-WIN11-26H1), HW-TIER-AMD.
- **Test Procedure:**
  1. Execute AetherShell on each OS build in the matrix; verify OS build and UBR detection via registry (`crates/aethershell-win/src/os_version.rs`).
  2. Boot on mixed-DPI multi-monitor setup (e.g. Primary 150% DPI, Secondary 100% DPI).
  3. Move active windows across displays and toggle display scaling in Windows Settings.
  4. Verify `WM_DPICHANGED` dynamically adjusts Direct2D render targets, recalculates font metrics, and repositions overlay surfaces via `SWP_NOZORDER | SWP_NOACTIVATE`.
  5. Detach secondary monitor, observe `WM_DISPLAYCHANGE`, re-attach monitor.
- **Pass Criteria:** Zero visual tearing or blurriness; overlay bounds conform exactly to virtual desktop work areas; Direct2D render targets recreate without memory leaks; unsupported features gracefully degrade to `native_noop`.

### Gate G03: Shell Isolation, Protected Process Non-Interference & Artifact Scan
- **Authoritative Requirement:** All stable binaries and packages contain zero Explorer extensions; no hooks into critical protected processes (`dwm.exe`, `csrss.exe`, `svchost.exe`, `audiodg.exe`, game processes, browsers).
- **Target Environment:** Clean disposable VMs, static package inspection.
- **Test Procedure:**
  1. Perform static binary analysis on compiled executables and libraries (`dumpbin /dependents`, PE import table inspection).
  2. Audit running process modules using `Get-Process explorer | Select-Object -ExpandProperty Modules` to prove zero AetherShell DLLs loaded in Explorer.
  3. Inspect system hook tables (`SetWindowsHookEx`) to verify no global system hooks are installed.
  4. Perform package artifact isolation audit (`scripts/audit_package_artifacts.ps1`) and regression suite (`tests/package_isolation_tests.ps1`) across compiled staging trees and portable ZIP packages to verify zero injected extensions, zero BHOs, zero detours, and clean path remapping.
  5. Perform module scan across all distribution packages (MSI, MSIX, Store layout, WinGet, portable ZIP).
- **Pass Criteria:** Exactly zero injected modules, detours, or hooks into Windows system processes; clean dependency scan conforming strictly to Apache-2.0.

### Gate G04: Taskbar Recovery, Capability Gating & Native Fallback
- **Authoritative Requirement:** External taskbar effects gated on capability allowlists; native fallback; dynamic Explorer crash/recovery handling; no repeated fighting or control loops.
- **Target Environment:** VM-WIN11-23H2, VM-WIN10-22H2 (Strict Disposable VMs per Q37:A).
- **Test Procedure:**
  1. Launch AetherShell with external taskbar styling enabled. Verify capability checks gate unsupported styling on modern Windows 11 XAML taskbars, selecting `NativeNoopBackend`.
  2. Simulate Explorer crash / termination: `taskkill /f /im explorer.exe`.
  3. Wait 3 seconds, then restart Explorer: `Start-Process explorer.exe`.
  4. Observe dynamic handling of `TaskbarCreated` message: tray icon re-registration, taskbar handle re-probe, and surface restacking.
- **Pass Criteria:** Zero tray icon duplication; clean recovery within 500ms; zero continuous reapplication loops or taskbar flicker; no crash dialogs.

### Gate G05: Experimental Qualification (Optional Explorer Extension)
- **Authoritative Requirement:** Any opt-in Explorer extension requires disposable-VM tests, then dedicated recoverable-hardware qualification before limited experimental distribution; marked `NOT_APPLICABLE` for stable core if absent.
- **Target Environment:** Disposable VM only (initial), dedicated recoverable physical target (secondary); strictly excluded from daily-use host (Q37:A, Q41:A).
- **Test Procedure:**
  1. Verify whether any in-process Explorer extension, XAML bridge DLL, or detour module is built or included in stable release artifacts.
  2. If absent: classify gate as `NOT_APPLICABLE` per PRD Section 18.2.
  3. If present in experimental branch: execute in-VM crash injection, Explorer restart recovery, clean uninstallation, and safe mode fallback before any limited distribution.
- **Pass Criteria:** Stable v1 core contains zero experimental extensions (`NOT_APPLICABLE`). Experimental builds require signed VM qualification report.

### Gate G06: Desktop Layering, Icon Passthrough & Win+D Resilience
- **Authoritative Requirement:** Wallpaper below passive widgets below desktop icons on qualifying builds; desktop icon lasso selection pass-through; safe fallback on unsupported builds.
- **Target Environment:** VM-WIN11-23H2, VM-WIN11-24H2, VM-WIN10-22H2 (Strict Disposable VMs per Q37:A).
- **Test Procedure:**
  1. Launch AetherShell in passive widget display mode. Verify overlay surface is parented/positioned behind `SHELLDLL_DefView` / `WorkerW` / `Progman` with `WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_LAYERED`.
  2. Perform mouse clicks and desktop icon lasso selection across widget areas. Verify clicks pass through to desktop icons without interception.
  3. Press `Win+D` (Show Desktop), minimize/restore all windows. Verify overlay maintains or restores correct Z-order behind desktop icons without obscuring them.
- **Pass Criteria:** Zero desktop icon click interference; lasso selection works smoothly; overlay surface correctly layers above wallpaper and below icons.

### Gate G07: Temporary Interactive Mode Hit-Testing & Safe Dismissal
- **Authoritative Requirement:** Tray/hotkey opens bounded overlay; user interactive actions execute; Escape/timeout/focus change restores passive icon input; strictly no `WS_EX_TOPMOST`.
- **Target Environment:** VM-WIN11-24H2, HW-TIER-AMD (attended non-invasive).
- **Test Procedure:**
  1. Trigger Interactive Mode via global hotkey `Win+Ctrl+A` (`HOTKEY_ID_INTERACTIVE_MODE = 0xAE01`) or tray context menu.
  2. Verify window styles clear `WS_EX_TRANSPARENT | WS_EX_NOACTIVATE` while strictly omitting `WS_EX_TOPMOST`.
  3. Click interactive widget regions (media controls, theme switch). Verify spatial pointer hit-testing dispatches action.
  4. Test all four mandatory auto-dismiss triggers:
     - Press `Escape` key.
     - Click outside active interactive regions onto desktop wallpaper.
     - Allow 8-second inactivity timeout to expire without cursor movement.
     - Switch focus to an external application window.
- **Pass Criteria:** All four dismissal triggers immediately restore `WS_EX_TRANSPARENT | WS_EX_NOACTIVATE` and restack behind desktop icons.

### Gate G08: Sensor Honesty, Fallback Values & Non-Waking Sampling
- **Authoritative Requirement:** Accurate native telemetry metrics; missing/unsupported counters collapse or show unknown; non-waking sampling intervals; never fabricated zeros.
- **Target Environment:** HW-TIER-AMD, VM-WIN11-24H2.
- **Test Procedure:**
  1. Query CPU (`GetSystemTimes`), RAM (`GlobalMemoryStatusEx`), storage (`GetDiskFreeSpaceExW`), battery (`GetSystemPowerStatus`), and network (`GetIfTable2`).
  2. Verify unsupported or uninstrumented metrics (e.g. GPU on non-admin/unsupported drivers, NPU) report explicit `Unavailable` status with technical reason, never fabricated `0.0`.
  3. Test sampling intervals across AC (1s) and DC battery (1s non-waking) power states.
- **Pass Criteria:** All metrics reflect truthful platform values; unavailable metrics clearly display fallback/unknown state in UI and diagnostic reports.

### Gate G09: Local Adaptive Audio, WASAPI Loopback & Power-Aware Framerate Pacing
- **Authoritative Requirement:** Default-on local WASAPI loopback; endpoint migration recovery; battery 15 FPS default, AC 30 FPS, configurable 60 FPS; strictly in-memory processing with no persistent audio samples.
- **Target Environment:** HW-TIER-AMD, HW-TIER-INTEL.
- **Test Procedure:**
  1. Play dynamic audio stream (Spotify, YouTube, media player). Verify live 512-point FFT spectrum bands update dynamically in presentation loop.
  2. Hot-unplug default audio output device or switch default endpoint. Verify `IMMNotificationClient::OnDefaultDeviceChanged` seamlessly migrates capture stream.
  3. Switch from AC power to DC battery power. Verify presentation loop immediately adapts framerate from 30/60 FPS down to 15 FPS with non-waking intervals.
  4. Verify audio samples exist strictly in volatile memory and are never written to disk or transmitted over network.
- **Pass Criteria:** Audio stream migrates within 250ms of endpoint change; framerate pacing conforms to active power profile; zero audio buffer underruns or memory leaks; zero audio persistence.

### Gate G10: Lua Worker Reliability, Supervised Job Containment & Continuous Trust Revalidation
- **Authoritative Requirement:** Lazy out-of-process script host in 128 MB Job Object (`CREATE_SUSPENDED`); SHA-256 digest authorization (`trusted_digests`); continuous watchdog integrity revalidation (nominal 2s interval) detecting file tampering, deletion, lock poisoning, or trust revocation fail-closed; malformed IPC payload rejection.
- **Target Environment:** VM-WIN11-24H2 (Strict Disposable VM per Q37:A).
- **Test Procedure:**
  1. Configure an active Lua widget profile with approved SHA-256 digest in `trusted_digests`. Verify worker spawns in suspended state, is assigned to 128 MB Job Object, and executes under Lua 5.4.
  2. **Fault Injection A (External Tampering):** While the worker is running, externally modify the script file on disk. Verify watchdog detects mismatch within the nominal 2-second tick, terminates worker subprocess fail-closed, and purges `script.*` bindings.
  3. **Fault Injection B (External Deletion):** Delete the active script file on disk. Verify worker is terminated fail-closed within the watchdog interval.
  4. **Fault Injection C (Abnormal Termination):** Kill `aethershell-script-host.exe` via Task Manager or `taskkill /f`. Verify Core detects exit, purges bindings, and increments crash counter without crashing Core.
  5. **Fault Injection D (Memory Quota):** Inject a runaway script attempting to allocate 200 MB RAM. Verify Windows Job Object terminates child worker at 128 MB limit; Core catches termination and enters quarantine.
  6. **Fault Injection E (Malformed IPC):** Send invalid, truncated, and unauthorized JSON payloads over Named Pipe. Verify server rejects invalid payloads fail-closed.
- **Pass Criteria:** Core tray process remains completely unaffected across all worker faults; no zombie processes; all bindings cleanly pruned; zero IPC denial-of-service.

### Gate G11: WinUI 3 Settings Independence, IPC Disconnect & Crash Isolation
- **Authoritative Requirement:** Separate C++/WinRT on-demand process; exits on close with 0 MB resident RAM; survives lost IPC; Core survives Settings crash.
- **Target Environment:** HW-TIER-INTEL, VM-WIN11-23H2.
- **Test Procedure:**
  1. Launch Settings UI from tray context menu (`aethershell-settings.exe`). Verify connection over authenticated Named Pipe IPC.
  2. Mutate configuration controls (theme switch, audio FPS mode). Verify atomic update in Core and persistent storage.
  3. Close Settings window. Verify process terminates cleanly and releases all resident memory (0 MB resident RAM).
  4. **Fault Injection:** Re-open Settings, simulate high-throughput IPC streaming, and forcefully kill `aethershell-settings.exe`.
- **Pass Criteria:** Core continues running with uninterrupted audio and desktop presentation; pipe server handles broken pipe gracefully and accepts subsequent reconnections.

### Gate G12: Configuration Schema Parsing, Versioned Migrations, Atomic Persistence & AppData Sharing
- **Authoritative Requirement:** TOML configuration parsing and validation (`ConfigValidationError`); bidirectional reversible schema migrations (`1.0.0 <-> 1.1.0 <-> 2.0.0`); non-destructive user customization preservation (`_preserved_runtime`); loader version negotiation (`is_supported_schema_version`); atomic write replacement (`ReplaceFileW`); fail-closed pre-migration backup (`.bak`); concurrent multi-instance Win32 exclusive file-locking (`FILE_SHARE_MODE(0)`) in canonical `%LOCALAPPDATA%\AetherShell`.
- **Target Environment:** Clean disposable VMs, isolated host test directories.
- **Test Procedure:**
  1. Parse all valid and invalid TOML sample configurations across versions 1.0.0, 1.1.0, and 2.0.0. Verify invalid configurations are rejected with descriptive errors.
  2. Mutate settings and execute atomic write persistence via `ReplaceFileW`. Verify file write atomicity and `.bak` backup creation.
  3. Execute bidirectional migrations forward and backward (`1.0.0 -> 1.1.0 -> 2.0.0 -> 1.1.0 -> 1.0.0`), asserting zero loss of custom user fields and successful post-migration application loading.
  4. Simulate sudden process termination during file write; verify config self-healing restores from backup.
  5. Execute installed and portable binaries concurrently with shared AppData store; verify exclusive file-locking conflict protection (`ConfigLockGuard` / `ERROR_SHARING_VIOLATION`).
- **Pass Criteria:** Zero configuration corruption; atomic persistence verified; backup rollback operates reliably; bidirectional migration round-trips succeed cleanly; shared AppData access complies with PRD Q38:B.

### Gate G13: Empirical Multi-Process Runtime Performance Qualification (PRD Section 14)
- **Authoritative Requirement:** Real measured workloads meet ratified PRD Section 14.2 budgets on baseline matrix; multi-process working set (`WorkingSetSize`) and private commit (`PrivateUsage`); normalized CPU; variance documented; honest GPU status; no false exact-zero claims.
- **Target Environment:** HW-TIER-INTEL, HW-TIER-AMD, HW-TIER-NVIDIA.
- **Test Procedure:**
  1. Deploy release binaries with `AETHERSHELL_APP_DATA_DIR` directed to isolated storage.
  2. Execute formal qualification benchmark:
     `aethershell.exe --benchmark-runtime minimal_idle --formal --repetitions 5 --duration 60`
  3. Execute 5-repetition 60-second evaluations for active profiles: `default_widgets`, `active_media_visualization`, `mixed_monitors`.
  4. Collect process working sets (`WorkingSetSize`), private commit (`PrivateUsage`), multi-core normalized CPU, and ETW presentation metrics.
- **Pass Criteria:** All measurements comply with ratified PRD Section 14.2 budgets (Minimal Idle: Core WS <= 30 MiB median, steady CPU <= 0.1%). Provisional proposals evaluated and recorded with variance.
- **Empirical Execution Record (HW-TIER-AMD / EVD-035):**
  - Executed formal 5x60s benchmark on physical AMD Ryzen 5 5500U, 16 GB RAM, AMD Radeon Graphics, Windows 11 Pro 26H2 Build 26300 under isolated AppData sandbox (0 items in `%LOCALAPPDATA%\AetherShell`).
  - Observed steady-state metrics (600 samples across 300s): Core resident WorkingSetSize median **43.8 MB** (p95: 44.0 MB, max: 44.2 MB; variance < 0.57 MB); Private Commit median **25.2 MB** (p95: 25.6 MB); Multi-core normalized CPU median **0.52%** (p95: 1.04%, max: 1.82%).
  - Full ledger: `docs/qualification/ryzen5_5500u_benchmark_ledger.json`.
  - Truthful disposition: Gate G13 remains `PARTIALLY_VERIFIED`. While private commit (25.2 MB) satisfies the <= 30 MB budget, resident physical WorkingSetSize (43.8 MB) and CPU (0.52%) exceed preliminary PRD Section 14.2 goals, providing the first authentic baseline to guide optimization or ratification under PRD Section 14.4.

### Gate G14: Accessibility, Keyboard Navigation, UI Automation & High-Contrast Themes
- **Authoritative Requirement:** Full keyboard navigation; native Windows UI Automation (UIA) COM provider hierarchy via `WM_GETOBJECT (0x003D)`; Win32 `NotifyWinEvent` focus/alert dispatch; Narrator screen reader speech; Windows high-contrast theme support; DPI/text scaling; reduced motion respect; localization truncation rules.
- **Target Environment:** VM-WIN11-24H2, VM-WIN10-22H2.
- **Test Procedure:**
  1. Enable Windows High Contrast mode (Desert, Aquatic, Dusk, Night Sky). Verify AetherShell theme engine applies high-contrast color tokens with compliant contrast ratios.
  2. Enable Windows "Make text bigger" (up to 200% text scaling). Verify widgets recalculate layout without clipped or overlapping text.
  3. Enable Windows "Show animations" off (reduced motion). Verify visual transitions and spectrum smoothing adapt to non-animated presentation.
  4. Audit keyboard navigation in Settings and Interactive Mode: Tab, Shift+Tab, Arrow keys, Enter, Space, Escape with focus ring invalidation.
  5. Inspect UIA accessibility tree using `Accessibility Insights for Windows` or `Inspect.exe`, verifying `WM_GETOBJECT` returns `IRawElementProviderSimple` and exposes full `IRawElementProviderFragment` hierarchy, standard properties (Name, ControlType, BoundingRectangle, HasKeyboardFocus, IsEnabled), and `IInvokeProvider`.
  6. Audit Windows Narrator audio announcements for focus changes and action invocations in clean VM.
- **Pass Criteria:** Full keyboard navigability; high-contrast readability; text scaling without truncation; UIA tree exposes actionable controls to screen readers; Narrator announces element names and values.

### Gate G15: Five-Channel Packaging Release Qualification (MSI, MSIX, Store, WinGet, Portable)
- **Authoritative Requirement:** MSI, MSIX, Microsoft Store approved listing, WinGet package, and self-contained portable fully qualified; no partial stable release (Q40:A); deferred during 0.1.0-dev development.
- **Target Environment:** Clean disposable VMs for all 5 channels; external Microsoft Store submission environment.
- **Test Procedure:**
  1. Build WiX v4 MSI installer; verify per-machine installation, upgrade, repair, and clean uninstallation in clean VM.
  2. Build MSIX package using `build-msix.ps1` with semantic validation; test package installation, update, and removal via App Installer.
  3. Verify self-contained portable ZIP layout operates without installation and respects canonical AppData per Q38:B.
  4. Validate WinGet manifest syntax and hash verification against built installer artifacts.
  5. Prepare Microsoft Store submission package and certification preflight.
- **Pass Criteria:** Clean installation, upgrade, and removal across all 5 channels; zero orphaned registry keys or files; Store certification passes. *Status: DEFERRED during 0.1.0-dev per Q40:A/Q46:B.*

### Gate G16: License, Privacy & Network Isolation Audit
- **Authoritative Requirement:** Apache-2.0 `LICENSE` and complete `NOTICE`; compiler path remapping (`--remap-path-prefix`, `/pathmap`); zero unsolicited analytics or audio upload; machine-readable SPDX-2.3 SBOM with `NOASSERTION` concluded licenses; dynamic packet-level NDIS capture audit (`scripts/audit_packet_level_network_isolation.ps1`) and continuous socket sampling + DNS cache differential proving 0 outbound packets/connections.
- **Target Environment:** VM-WIN11-24H2, isolated host test directories.
- **Test Procedure:**
  1. Execute `scripts/generate_sbom.ps1 -Strict` to verify 100% permissive dependencies and SPDX-2.3 JSON schema compliance.
  2. Execute `scripts/audit_runtime_network_isolation.ps1 -Strict -SpawnTestProcess` to audit active socket tables on live instances.
  3. Execute `scripts/audit_packet_level_network_isolation.ps1 -SpawnAllComponents -DurationSeconds 3 -Strict` to perform continuous 25ms socket sampling and DNS client cache differential analysis across Core, Settings, and Script Host.
  4. In clean disposable VM, execute NDIS packet capture via `pktmon.exe` (`audit_packet_level_network_isolation.ps1 -RequirePktMon -Strict`) to capture all network adapter traffic during 60 minutes of live execution.
  5. Audit captured packets for DNS queries, HTTP/HTTPS requests, or outbound telemetry packets.
  6. Verify compiler path remapping eliminates developer paths from release binaries.
- **Pass Criteria:** Exactly zero outbound network packets generated; zero sockets opened; zero DNS queries; binary strings reveal no internal paths; SBOM and license attributions 100% compliant with Apache-2.0.

### Gate G17: Safe Mode, Failure Injection Recovery & Subsystem Quarantine
- **Authoritative Requirement:** `--safe-mode` command-line switch forces `NativeNoopBackend` and bypasses hooks; widget/worker crash quarantine; Explorer recovery; graphics driver TDR / device loss exponential backoff recovery; IPC and update recovery.
- **Target Environment:** VM-WIN11-23H2 (Strict Disposable VM per Q37:A).
- **Test Procedure:**
  1. Launch `aethershell.exe --safe-mode`. Verify core initializes with `NativeNoopBackend`, bypasses all shell hooks, and starts tray interface.
  2. Trigger repeated worker crashes. Verify supervisor increments crash count and transitions to `SupervisorState::Quarantined` without taking down Core.
  3. Simulate Direct2D / DXGI device removal (`DXGI_ERROR_DEVICE_REMOVED` / `DXGI_ERROR_DEVICE_RESET`). Verify engine executes bounded exponential backoff recovery and recreates swapchain/render target.
  4. Corrupt `config.toml` with garbage data. Verify core detects corruption and recovers clean configuration from `.bak` backup.
- **Pass Criteria:** Engine survives simulated faults; safe mode operates deterministically; GPU device loss recovers cleanly without infinite recursion; corrupted configs recover safely.

### Gate G18: Strict No-Go Criteria, Evidence Provenance & Release Ledger Sign-Off
- **Authoritative Requirement:** No unverified critical behavior represented as supported; no disruptive physical-host tests (Q37:A); formal release ledger with signed artifact hashes and independent reviewer sign-off before stable release.
- **Target Environment:** Full qualification repository and artifact audit.
- **Test Procedure:**
  1. Audit all 20 gates in `docs/STATUS.md` and `docs/EVIDENCE.md` against authoritative PRD requirements. Verify zero unsubstantiated `VERIFIED` claims.
  2. Confirm all disruptive tests executed strictly inside verified disposable VMs with rollback verification.
  3. Record full Git commit SHAs, binary SHA-256 hashes, test timestamps, environment metadata, and reviewer signatures for every test record.
  4. Verify `CORE_STABLE_CANDIDATE` is strictly not declared until all applicable gates satisfy empirical criteria.
- **Pass Criteria:** 100% truthful gate accounting; zero unverified assertions; complete cryptographic artifact provenance; explicit owner sign-off.

### Gate G19: Coexistence Qualification with Competing Shell Customizers
- **Authoritative Requirement:** Detected competing taskbar style controllers (`TranslucentTB`, `StartAllBack`, `ExplorerPatcher`, `RetroBar`) cause deterministic yield to `NativeNoopBackend` (Q21:A) without terminating or modifying third-party processes.
- **Target Environment:** VM-WIN11-23H2 (Strict Disposable VM per Q37:A).
- **Test Procedure:**
  1. Install competing shell customizers in VM: `TranslucentTB`, `StartAllBack`, and `RetroBar`.
  2. Launch competing utilities, then launch AetherShell.
  3. Verify `detect_taskbar_conflicts()` identifies competing processes and deterministically yields to `NativeNoopBackend` per PRD Q21:A.
- **Pass Criteria:** Zero process termination or injection into competing utilities; zero taskbar visual flickering or competing render loops.

### Gate G20: Packaging Identity, Cross-Channel AppData Continuity & Clean Rollback
- **Authoritative Requirement:** Canonical AppData consistency (`%LOCALAPPDATA%\AetherShell`) across all five deployment channels (MSI, MSIX, Store, WinGet, portable) adhering to Q38:B; clean startup toggle coordination; safe upgrade and rollback without deleting user data.
- **Target Environment:** Clean disposable Windows VMs.
- **Test Procedure:**
  1. Create user configuration and widget layouts in MSI deployment.
  2. Install MSIX package; verify MSIX resolves to canonical `%LOCALAPPDATA%\AetherShell` and loads existing user data seamlessly.
  3. Launch portable executable; verify identical configuration access without data duplication or migration loss via `tests/packaging_identity_continuity_tests.ps1`.
  4. Test startup toggle coordination: toggling "Start with Windows" in any package channel cleanly manages registry / startup task without conflicts.
  5. Execute uninstallation and rollback: verify application binaries are removed while user data in `%LOCALAPPDATA%\AetherShell` is preserved intact.
- **Pass Criteria:** Identical configuration state shared across all channels; startup settings coordinated; uninstall preserves user data; rollback operates cleanly without data loss.

---

## 4. Host Safety Preflight, Resource Budgets & Isolation Guard Architecture

### 4.1 Binding Operating Authority & Owner Approvals

| Decision | Approved | Binding Operational Scope |
|---|---|---|
| **Q01:B** | Approved | Existing Windows development PC authorized to host isolated disposable VMs, **strictly one VM at a time**, following safety and resource preflight. |
| **Q02:A** | Approved | Full disruptive qualification authorized **inside verified disposable VMs only**, including controlled Explorer restarts, process termination, fault injection, installer tests, and checkpoint recovery. |
| **Q03:A** | Approved | Non-invasive physical AMD laptop performance measurements authorized during **owner-attended session only**; no disruptive host experimentation. |
| **Q04:A** | Approved | Sequential owner-controlled PR review and merge governance: PR #51 first, then PR #52, with separate explicit approval required before each merge. |
| **Q47:A** | Approved | Non-invasive G13 runtime resource optimizations authorized: event-driven `GetMessageW` tray thread (0 wakeups/CPU when idle), Direct2D dirty-state presentation gating (skips redundant scene compilation and DXGI swapchain draw calls during silence), and WASAPI shared-mode idle backoff (250ms). |
| **Q48:A** | Approved | Owner explicitly authorizes hosting a single lightweight disposable VM on the local Windows PC (allocating 2 GB RAM / 2 vCPUs for a single disposable Hyper-V or Windows Sandbox guest), adhering strictly to safety preflight constraints (Q01:B, Q02:A, Q37:A). |

### 4.2 Host Resource Safeguards (16 GB Physical RAM Envelope)

Because the development PC possesses 16 GB physical RAM (~15.3 GB visible) with active background development workloads (WSL2, VS Code, Antigravity, Memory Compression), host resource safety is paramount:

1. **Single-VM Concurrency:** Strictly **one guest VM** active at any given time. Concurrent multi-VM execution is prohibited.
2. **Per-VM Resource Budget (Q48:A):**
   - **vCPU Allocation:** Maximum 2 vCPUs (out of 12 logical processors).
   - **RAM Allocation:** Allocated 2048 MB (2 GB) RAM for the single disposable Hyper-V or Windows Sandbox guest.
   - **Disk Allocation:** Maximum 25 GB dynamic VHDX on Volume C: (ensuring host free space remains > 35 GB at all times).
3. **Host Safety Preconditions:**
   - Host free physical memory must be >= 2048 MB before launching any VM.
   - Host disk free space on Volume C: must be >= 40 GB before creating VM disks or checkpoints.
   - Host CPU utilization must be < 50% steady-state before beginning qualification runs.
   - If host resources are insufficient, the test must be marked `BLOCKED_RESOURCE_CAPACITY`. Never force execution or terminate user applications to free memory.
4. **Approval Boundary Checkpoints:**
   - The owner's Q01:B approval authorizes using the PC as a VM host **if compatible and safe**.
   - It does **NOT** authorize: host reboots, hypervisor installation, Hyper-V feature enablement, Windows security configuration modifications, storage repartitioning, or unapproved ISO downloads.
   - Any operation requiring host modification or elevation must stop at the boundary and request explicit owner-attended authorization.

### 4.3 Mandatory Host-vs-Guest Isolation Guard

Every disruptive command (e.g. `taskkill /f /im explorer.exe`, process aborts, driver resets, installer executions) must employ an automated machine-identity guard that **fails closed**. In strict compliance with Part I Section 5.3, the guard **never relies solely on a shell prompt, window title, working directory, or environment variable (`$env:COMPUTERNAME`)**:

```powershell
# Mandatory Host vs Guest Precondition Guard (Fails closed on physical host)
$ExpectedGuestComputerName = "AETHERSHELL-VM"

# 1. Non-spoofable hardware query via CIM (cannot be bypassed by modifying environment variables)
$systemInfo = Get-CimInstance Win32_ComputerSystem -ErrorAction Stop
$biosInfo = Get-CimInstance Win32_BIOS -ErrorAction Stop

$isVirtualMachine = ($systemInfo.Model -match 'Virtual|Hyper-V|VMware|KVM') -or 
                    ($systemInfo.Manufacturer -match 'Microsoft Corporation|VMware|QEMU') -or
                    ($biosInfo.SerialNumber -match 'Microsoft|VMware')

if (-not $isVirtualMachine) {
    Write-Error "CRITICAL SAFETY VIOLATION: Disruptive command blocked! Detected physical host machine (Model: '$($systemInfo.Model)', Manufacturer: '$($systemInfo.Manufacturer)'). Disruptive shell tests are strictly VM-only per Q37:A. Aborting immediately."
    exit 1
}

# 2. Strict guest machine name verification
if ($systemInfo.Name -ne $ExpectedGuestComputerName) {
    Write-Error "CRITICAL SAFETY VIOLATION: Guest machine identity mismatch! Expected '$ExpectedGuestComputerName', detected '$($systemInfo.Name)'. Aborting immediately."
    exit 1
}
```

### 4.4 VM Checkpoint, Execution & Rollback Protocol

Before each disruptive test group:
1. Verify target machine identity is the intended guest VM (`AETHERSHELL-VM`).
2. Verify clean baseline snapshot exists (`Checkpoint-VM -Name $VmName -SnapshotName "PreTest_CleanBaseline"`).
3. Confirm adequate host storage for checkpoint growth.
4. Execute test procedure inside guest VM.
5. Collect logs, traces, and screenshots to dedicated external test-artifact directory (`target/qualification_artifacts/`).
6. Restore clean baseline snapshot (`Restore-VMSnapshot -Name $VmName -Name "PreTest_CleanBaseline"`).
7. Confirm guest returns to healthy baseline and host remains completely unaltered.

### 4.5 Owner-Attended Physical AMD Measurement Protocol (Q03:A)

For physical AMD laptop measurements:
1. Session must be owner-attended; no unattended host benchmark execution.
2. Direct all application state to isolated directory: `$env:AETHERSHELL_APP_DATA_DIR = "$PWD\target\qualification_artifacts"`.
3. Read-only performance counters and ETW traces only.
4. Strictly prohibited on physical host: Explorer restarts, taskbar styling hooks, window reparenting, graphics driver restarts, TDR injection, game anti-cheat interactions.
5. Record truthful hardware metrics (AMD Ryzen 5 5500U, AMD Radeon Graphics WDDM 3.1 Driver 31.0.21925.1001, 16 GB RAM); report uninstrumented metrics as `Unavailable`.

---

## 5. Test Execution Governance & Evidence Artifacts

1. **Pre-Execution Check:** Every test run must record environment metadata (OS build, UBR, GPU driver version, hardware specs, Git candidate SHA, binary SHA-256 hash).
2. **Artifact Packaging:** Test logs, ETW traces (`.etl`), Wireshark captures (`.pcapng`), and benchmark ledgers (`.json`) must be archived in `qualification/evidence/<test-id>/`.
3. **Impartial Adversarial Audit:** All test execution reports must be audited by an independent reviewer subagent before updating any gate status.
4. **Owner Sign-Off:** In accordance with PRD Section 18.2 and Owner Decisions Q04:A / Q44:A, formal promotion of any gate to `VERIFIED` and declaration of `CORE_STABLE_CANDIDATE` requires explicit owner authorization.

