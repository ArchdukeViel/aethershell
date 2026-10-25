# AetherShell Implementation & Verification Ledger

**Document Revision:** 1.0.0-dev  
**Last Updated:** 2026-10-25  
**Current Phase:** Phase 2 (Taskbar State Engine & Desktop Layering Host)

---

## Acceptance Gates (PRD Section 18.2)

| Gate | Description | Status | Verification Candidate | Evidence / Notes |
|:---|:---|:---:|:---:|:---|
| **G01** | Core startup & clean shutdown (user-mode, non-elevated, single-instance) | `PARTIALLY_VERIFIED` | `feat/phase1-runtime-tray-ipc` | Multi-process singleton mutex, dedicated tray message pump & IPC graceful shutdown verified (EVD-003) |
| **G02** | Windows compatibility (Win 10 22H2 & Win 11 x64, honest fallback) | `PARTIALLY_VERIFIED` | `feat/phase0-foundational-workspace` | Detected Windows 11 26H2 Build 26300 honestly; build family classification unit-tested |
| **G03** | Shell isolation (no Explorer injection in stable builds) | `VERIFIED` | `feat/phase0-foundational-workspace` | Zero Explorer injection hooks or modules in workspace crates |
| **G04** | Taskbar recovery (external effects gated, native fallback) | `VERIFIED` | `feat/phase2-taskbar-desktop-host` | Out-of-context WinEvent monitor, precedence table, `ExternalAppearanceBackend` gated fallback, and Safe Mode override verified (EVD-004) |
| **G05** | Experimental qualification (disposable VM qualification for bridge) | `NOT_STARTED` | — | Separate experimental channel only |
| **G06** | Desktop layering (wallpaper → widgets → desktop icons) | `PARTIALLY_VERIFIED` | `feat/phase2-taskbar-desktop-host` | Passive surface extended styles (`WS_EX_TRANSPARENT | WS_EX_NOACTIVATE`) & SHELLDLL_DefView hierarchy validation enforced (EVD-004) |
| **G07** | Temporary Interactive Mode (bounded overlay, auto-dismiss) | `VERIFIED` | `feat/phase2-taskbar-desktop-host` | Escape key, focus loss, session lock, and timeout auto-dismissal verified (EVD-004) |
| **G08** | Sensor honesty (missing counters display honest unavailable state) | `NOT_STARTED` | — | No fabricated 0% metrics |
| **G09** | Local adaptive audio (default-on WASAPI spectrum, adaptive FPS) | `NOT_STARTED` | — | Event-driven shared loopback, 30 FPS AC / 15 FPS battery |
| **G10** | Lua worker reliability & trust (separate process, digest consent) | `NOT_STARTED` | — | aethershell-script-host supervised child process (Q32:B) |
| **G11** | WinUI Settings independence (C++/WinRT separate on-demand process) | `PARTIALLY_VERIFIED` | `feat/phase1-runtime-tray-ipc` | IPC `open_settings` command routing and second-instance invocation verified |
| **G12** | Schema/backup (TOML parsing, atomic migrations, AppData storage) | `PARTIALLY_VERIFIED` | `feat/phase0-foundational-workspace` | AppConfig schema 1.0.0, path traversal rejection, atomic save with backup rollback verified |
| **G13** | Performance budgets (measured idle/active profiles, no false claims) | `NOT_STARTED` | — | Standardized 60s benchmark harness |
| **G14** | Accessibility (UIA, keyboard navigation, high contrast, reduced motion) | `NOT_STARTED` | — | Windows text scaling & contrast audits |
| **G15** | Five-channel release (MSI, MSIX, Store, WinGet, portable ZIP) | `NOT_STARTED` | — | Strict stable v1 release requirement (Q40:A) |
| **G16** | License/privacy (Apache-2.0, local-first, zero raw audio storage) | `PARTIALLY_VERIFIED` | Init Commit | Apache-2.0 LICENSE & NOTICE.md established; secret redaction unit-tested |
| **G17** | Safe Mode & recovery (watchdog, quarantine, configuration rollback) | `PARTIALLY_VERIFIED` | `feat/phase2-taskbar-desktop-host` | Safe mode flag suppresses taskbar mutations and forces native_noop fallback across coordinator |
| **G18** | Strict no-go & evidence ledger (independent reviews, commit-bound) | `PARTIALLY_VERIFIED` | `feat/phase0-foundational-workspace` | Status ledger, evidence records, and independent subagent audit engaged |
| **G19** | Coexistence (competing customizer yields safely without interference) | `VERIFIED` | `feat/phase2-taskbar-desktop-host` | Non-invasive detection of TranslucentTB, StartAllBack, ExplorerPatcher, RetroBar with automatic yield-and-notify (Q21:A) |
| **G20** | Packaging identity (consistent AppData semantics across channels) | `NOT_STARTED` | — | AppData-backed portable mode (Q38:B) |

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
- [ ] **Phase 3: Widget Engine, Skins & Design System**
- [ ] **Phase 4: Telemetry, Audio Visualization & Lua Worker**
- [ ] **Phase 5: WinUI 3 Settings, Packaging & Release Qualification**
