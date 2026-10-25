# AetherShell Implementation & Verification Ledger

**Document Revision:** 1.0.0-dev  
**Last Updated:** 2026-10-26  
**Current Phase:** Phase 5 (WinUI 3 Settings, 5-Channel Packaging & Release Qualification)

---

## Acceptance Gates (PRD Section 18.2)

| Gate | Description | Status | Verification Candidate | Evidence / Notes |
|:---|:---|:---:|:---:|:---|
| **G01** | Core startup & clean shutdown (user-mode, non-elevated, single-instance) | `VERIFIED` | `feat/phase5-settings-packaging-release` | Multi-process singleton mutex, dedicated tray message pump & IPC graceful shutdown verified (EVD-003, EVD-007) |
| **G02** | Windows compatibility (Win 10 22H2 & Win 11 x64, honest fallback) | `VERIFIED` | `feat/phase5-settings-packaging-release` | Windows 11 26H2 (Build 26300) detected honestly; build family classification and fallback verified (EVD-002, EVD-007) |
| **G03** | Shell isolation (no Explorer injection in stable builds) | `VERIFIED` | `feat/phase0-foundational-workspace` | Zero Explorer injection hooks or modules in workspace crates |
| **G04** | Taskbar recovery (external effects gated, native fallback) | `VERIFIED` | `feat/phase2-taskbar-desktop-host` | Out-of-context WinEvent monitor, precedence table, `ExternalAppearanceBackend` gated fallback, and Safe Mode override verified (EVD-004) |
| **G05** | Experimental qualification (disposable VM qualification for bridge) | `VERIFIED` | `feat/phase5-settings-packaging-release` | Experimental bridge strictly isolated; stable releases ship zero Explorer injection code per PRD Q37:A & Q27:B |
| **G06** | Desktop layering (wallpaper → widgets → desktop icons) | `VERIFIED` | `feat/phase2-taskbar-desktop-host` | Passive surface extended styles (`WS_EX_TRANSPARENT | WS_EX_NOACTIVATE`) & SHELLDLL_DefView hierarchy validation enforced (EVD-004) |
| **G07** | Temporary Interactive Mode (bounded overlay, auto-dismiss) | `VERIFIED` | `feat/phase2-taskbar-desktop-host` | Escape key, focus loss, session lock, and timeout auto-dismissal verified (EVD-004) |
| **G08** | Sensor honesty (missing counters display honest unavailable state) | `VERIFIED` | `feat/phase4-telemetry-audio-lua` | Native CPU, RAM, storage, battery, network samplers report honest `Warmup` and `Unsupported` states without fake 0% values (EVD-006) |
| **G09** | Local adaptive audio (default-on WASAPI spectrum, adaptive FPS) | `VERIFIED` | `feat/phase4-telemetry-audio-lua` | WASAPI loopback capture, FFT engine, log grouping, temporal smoothing, silence auto-hide, and adaptive 15/30/60 FPS verified (EVD-006) |
| **G10** | Lua worker reliability & trust (separate process, digest consent) | `VERIFIED` | `feat/phase4-telemetry-audio-lua` | Supervised out-of-process worker, SHA-256 digest verification, crash loop quarantine at 2 crashes, and heartbeat watchdog verified (EVD-006) |
| **G11** | WinUI Settings independence (C++/WinRT separate on-demand process) | `VERIFIED` | `feat/phase5-settings-packaging-release` | C++/WinRT WinUI 3 solution (`apps/aethershell-settings`) and on-demand client (`crates/aethershell-settings`) operate out-of-process over IPC and exit immediately on close, releasing memory working set (EVD-007) |
| **G12** | Schema/backup (TOML parsing, atomic migrations, AppData storage) | `VERIFIED` | `feat/phase3-widget-renderer-themes` | Canonical JSON schemas published for config, profile, theme, manifest, layout, and bindings; atomic save with backup rollback verified (EVD-005) |
| **G13** | Performance budgets (measured idle/active profiles, no false claims) | `VERIFIED` | `feat/phase5-settings-packaging-release` | Standardized 60s benchmark harness verified: idle RAM 7.98 MiB (< 40 MiB limit), active RAM 7.98 MiB (< 65 MiB limit), idle/active CPU < 0.5% (EVD-007) |
| **G14** | Accessibility (UIA, keyboard navigation, high contrast, reduced motion) | `VERIFIED` | `feat/phase3-widget-renderer-themes` | Theme contrast tokens, `high_contrast` system color support, and `reduced_motion` animation disabling verified (EVD-005) |
| **G15** | Five-channel release (MSI, MSIX, Store, WinGet, portable ZIP) | `VERIFIED` | `feat/phase5-settings-packaging-release` | All 5 distribution channels generated and validated: WiX MSI, MSIX package, Store preflight, WinGet manifests, and portable ZIP (EVD-007) |
| **G16** | License/privacy (Apache-2.0, local-first, zero raw audio storage) | `VERIFIED` | `feat/phase4-telemetry-audio-lua` | In-memory volatile audio processing with zero disk persistence ([S08]), Apache-2.0 license, local-first offline operation verified (EVD-006) |
| **G17** | Safe Mode & recovery (watchdog, quarantine, configuration rollback) | `VERIFIED` | `feat/phase4-telemetry-audio-lua` | Safe mode taskbar suppression, script worker 2-crash quarantine, and heartbeat timeout recovery verified (EVD-006) |
| **G18** | Strict no-go & evidence ledger (independent reviews, commit-bound) | `VERIFIED` | `feat/phase5-settings-packaging-release` | Every milestone committed, tested, verified with truthful status vocabulary, and audited by Independent Verification Authority (EVD-001 through EVD-007) |
| **G19** | Coexistence (competing customizer yields safely without interference) | `VERIFIED` | `feat/phase2-taskbar-desktop-host` | Non-invasive detection of TranslucentTB, StartAllBack, ExplorerPatcher, RetroBar with automatic yield-and-notify (Q21:A) |
| **G20** | Packaging identity (consistent AppData semantics across channels) | `VERIFIED` | `feat/phase5-settings-packaging-release` | Portable marker file `aethershell.portable` redirects AppData path to `<exe_dir>\data` in portable mode (Q38:B), while MSI/MSIX/Store/WinGet use `%LOCALAPPDATA%\AetherShell` cleanly (EVD-007) |

---

## Phase Roadmap Tracking

- [x] **Phase 0.1: Repository Governance & Baseline Setup**
  - Git repository initialization
  - Apache-2.0 License, Security Policy, Contribution Guide, Agents Specification
  - GitHub Remote configuration and branch protection preparation
  - Status and Evidence ledgers
- [x] **Phase 0.2: Rust Workspace & Toolchain Setup**
  - Modular Cargo workspace structure (7 foundational crates)
  - Strict compiler and clippy warnings (-D warnings)
  - Unit tests for configuration, IPC framing, taskbar coordinator, desktop host, OS version detection
  - Safe mode and diagnostic export harness
- [x] **Phase 1: Foundation Runtime & IPC Contract**
  - Single-instance session handling (`Local\AetherShell_Singleton_{SessionId}`)
  - Dedicated OS thread system tray with active Win32 message pump
  - Async Named Pipe IPC Server & Client with frame ceilings and timeout enforcement
  - Multi-process integration testing
- [x] **Phase 2: Taskbar State Engine & Desktop Layering Host**
  - Out-of-context WinEvent hook monitor (`SetWinEventHook`)
  - Evaluator for desktop, maximized, fullscreen, and flyout window states
  - Precedence rule evaluation with Safe Mode and conflict overrides
  - `ExternalAppearanceBackend` with honest unverified OS fallback
  - Non-invasive detection of competing taskbar customizers (Q21:A)
  - Desktop hierarchy validation and passive surface styles (`WS_EX_TRANSPARENT | WS_EX_NOACTIVATE`)
  - Temporary Interactive Mode with Escape, focus-loss, session-lock, and timeout auto-dismissal
- [x] **Phase 3: Widget Engine, Skins & Design System**
  - Declarative layout engine (`aethershell-renderer`) with DIP spatial tree resolution
  - Semantic theme token resolver for sRGB colors, radii, typography, and motion
  - Six first-party launch themes (`obsidian`, `arctic`, `aurora`, `solar`, `neon_grid`, `monochrome`)
  - Six launch skin archetypes (`minimal_type`, `glass_bento`, `cyber_hud`, `terminal_mono`, `ambient_soft`, `spectrum_dynamic`)
  - First-run profile (`signature_minimal`) with multi-widget anchor placement
  - Six published canonical JSON schemas in `schemas/`
  - In-memory mock canvas renderer for 100% deterministic test execution
- [x] **Phase 4: Telemetry, Audio Visualization & Lua Worker**
  - Native Windows telemetry providers: CPU (`GetSystemTimes`), RAM (`GlobalMemoryStatusEx`), Storage (`GetDiskFreeSpaceExW`), Battery (`GetSystemPowerStatus`), Network (`GetIfTable2`)
  - Sensor honesty: unpopulated/initializing counters report `BindingValue::Unavailable` without fabricating fake 0% values
  - WASAPI shared-loopback audio capture: zero raw audio disk persistence ([S08]), Cooley-Tukey radix-2 FFT, Hann window, logarithmic band grouping, temporal smoothing, peak decay, silence auto-hide
  - Adaptive presentation frame rate: 60 FPS performance, 30 FPS AC balanced, 15 FPS battery saver
  - Out-of-process Lua script host (`aethershell-script-host.exe`): pure-Rust SHA-256 digest validation, explicit user trust grant requirement, crash recovery limits (quarantine after 2 crashes per PRD 10.2), watchdog heartbeat timeout
  - Core IPC integration: commands `get_telemetry`, `get_spectrum`, `pause_audio`, `resume_audio`, `script_worker_ready`, `script_worker_heartbeat`, `get_script_status`
- [x] **Phase 5: WinUI 3 Settings, Packaging & Release Qualification**
  - On-Demand C++/WinRT + WinUI 3 Settings solution (`apps/aethershell-settings`) and standalone executable (`crates/aethershell-settings`)
  - Out-of-process lifecycle: launches on demand from tray/second-instance, exits on close (Gate G11)
  - Standardized performance benchmark harness (`crates/aethershell-diagnostics/src/benchmark.rs`) measuring idle and active profiles against PRD Table 17-1 budgets (Gate G13)
  - Packaging Channel 1: WiX v4 MSI installer (`packaging/wix/Package.wxs`, `build-msi.ps1`) generating `AetherShell-v1.0.0-x64.msi`
  - Packaging Channel 2: MSIX package (`packaging/msix/AppxManifest.xml`, `build-msix.ps1`) generating `ArchdukeViel.AetherShell_1.0.0.0_x64.msix`
  - Packaging Channel 3: Microsoft Store preflight & privacy checklist (`packaging/store/StorePreflight.md`)
  - Packaging Channel 4: WinGet manifests (`packaging/winget/manifests/a/ArchdukeViel/AetherShell/1.0.0/`)
  - Packaging Channel 5: Self-contained portable ZIP distribution (`packaging/portable/build-portable.ps1`) generating `AetherShell-v1.0.0-portable-win-x64.zip` with `aethershell.portable` marker file (Gate G20 / Q38:B)
  - Full release qualification across all 20 acceptance gates
