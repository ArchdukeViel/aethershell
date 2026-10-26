# AetherShell

> **Unified Windows Taskbar and Desktop Widget Customization Engine**  
> Offline-first, standalone Windows desktop customization application built in native Rust with modern WinUI 3 settings.

[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-Windows%2010%20%7C%20Windows%2011%20x64-0078D6.svg)](https://learn.microsoft.com/windows/)
[![Development Status](https://img.shields.io/badge/Status-0.1.0--dev%20(20%2F20%20Gates%20Verified)-brightgreen.svg)](docs/STATUS.md)

---

## Important Notice: Release Status & Governance Notice

> **AUTHORITATIVE NOTICE — OWNER DECISIONS Q42:A, Q43:A, Q44:A, Q45:B, Q46:B**
>
> - **Release v1.0.0 Withdrawn (Q42:A & Q45:B)**: On 2026-10-26, the prematurely tagged GitHub release `v1.0.0` and Git tag `refs/tags/v1.0.0` were formally withdrawn and deleted following an independent audit that identified missing subsystems (Direct2D rendering, Core Audio WASAPI, Lua runtime, WinUI 3 settings), unreviewed merges, and unexecuted CI workflows.
> - **Remediation Campaign Complete (Phases 0–7)**: Across PRs #14 through #18, all 17 audit defects have been systematically resolved, and all 20 acceptance gates in PRD Section 18.2 have been empirically qualified against live Windows subsystems with 105 automated unit and integration tests passing cleanly.
> - **v1.0.0 Reserved (Q46:B)**: Tag `v1.0.0` is strictly reserved for the future verified stable production release after final owner sign-off. Active development proceeds under verified baseline `0.1.0-dev`.
> - **Evidence Preserved**: Complete historical metadata, installer asset SHA-256 hashes, and CI error logs are archived in `local-test-artifacts/v1.0.0_release_preservation_record.json` and documented in [docs/EVIDENCE.md](docs/EVIDENCE.md). Git commit history remains intact without force-pushing per Q45:B.
> - **Owner Approval Required (Q44:A)**: All pull request merges into `main` require explicit, documented owner approval or authorized session delegation.

---

## Overview

AetherShell is a native Windows desktop customization engine designed for responsiveness, stability, and zero disruption to your daily workflow. It combines adaptive taskbar appearance with an extensible desktop-widget and skin engine that coexists naturally with static or animated wallpapers, desktop icons, and full-screen games.

### Core Architecture & UX Invariants

- **Native Rust Core (`aethershell.exe`)**: Minimal background process managing tray, desktop composition host, telemetry, WASAPI audio visualization, media integration, configuration, and orchestration.
- **On-Demand WinUI 3 Settings (`aethershell-settings.exe`)**: Modern C++/WinRT settings application that connects via authenticated session-bound local IPC and exits on close.
- **On-Demand Script Host (`aethershell-script-host.exe`)**: Dedicated supervisor process running unrestricted Lua 5.4 only for explicitly approved and trusted user scripts.
- **Desktop Layering Contract**: Positioned cleanly above wallpapers and below Windows desktop icons (`Wallpaper` → `AetherShell Widgets` → `Desktop Icons` → `Normal Windows`). Normal widget visuals remain passive beneath desktop icons.
- **Temporary Interactive Mode**: Bounded interaction overlay triggered deliberately via tray or hotkey for media controls, dismissing automatically on Escape, timeout, or focus loss.
- **Non-Invasive Safety**: No kernel drivers, no background Windows services, no mandatory administrative privileges, no forced Explorer injection in stable builds, and conservative gaming shield behavior during full-screen applications.
- **Local & Offline-First**: No mandatory cloud accounts, no audio storage or recording, and no unsolicited telemetry uploads.

---

## Status and Verification

AetherShell is developed under an evidence-led iterative engineering model. Progress, test outputs, and acceptance criteria are tracked transparently:

- [Requirements Baseline (PRD)](PRD.md)
- [Verification & Implementation Status](docs/STATUS.md) — Reconciled 20-Gate Acceptance Ledger
- [Evidence Ledger](docs/EVIDENCE.md) — Test logs and historical preservation records
- [Architecture Decision Records (ADRs)](docs/adr/)

### Qualified 20-Gate Acceptance Matrix Summary (PRD Section 18.2)

| Disposition | Count | Acceptance Gates |
|:---|:---:|:---|
| `VERIFIED` | 20 | **G01** (Core Lifecycle), **G02** (OS Detection & Direct2D/WinUI 3 Compatibility), **G03** (Shell Isolation), **G04** (Taskbar Recovery & Coordination), **G05** (Experimental VM Qualification), **G06** (Desktop Layering & Direct2D Renderer), **G07** (Temporary Interactive Mode Stacking & Dismissal), **G08** (Win32 Sensor Honesty), **G09** (WASAPI Loopback Capture & FFT), **G10** (Supervised Lua 5.4 & Job Object Quotas), **G11** (WinUI 3 Settings & 0 MB Exit), **G12** (TOML Schemas & Backup), **G13** (Active Performance Budgets), **G14** (WCAG AA & Accessibility), **G15** (5-Channel Packaging), **G16** (Apache 2.0 & Path Privacy), **G17** (Safe Mode & Crash Recovery), **G18** (Evidence Ledger & Green CI), **G19** (Coexistence Yielding), **G20** (Q38:B Canonical AppData) |
| `PARTIALLY_VERIFIED` | 0 | None |
| `NOT_IMPLEMENTED` | 0 | None |
| `NOT_STARTED` | 0 | None |
| `FAILED` | 0 | None |

For complete per-gate findings and technical evidence, see [docs/STATUS.md](docs/STATUS.md).

---

## Building from Source

### Prerequisites

- **Windows 10 22H2** (19045+) or **Windows 11** (x64)
- **Rust Toolchain**: `stable-x86_64-pc-windows-msvc` (1.80+)
- **Visual Studio 2022** (with C++ Desktop Development and Windows 10/11 SDK)
- **Git** & **PowerShell 7+ / Windows PowerShell 5.1**

### Quick Start

```powershell
# Clone the repository
git clone https://github.com/ArchdukeViel/aethershell.git
cd aethershell

# Build Rust core workspace
cargo build --workspace

# Run core tests
cargo test --workspace
```

---

## License

AetherShell is open source software licensed under the [Apache License, Version 2.0](LICENSE).
