# AetherShell

> **Unified Windows Taskbar and Desktop Widget Customization Engine**  
> Offline-first, standalone Windows desktop customization application built in native Rust with modern WinUI 3 settings.

[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-Windows%2010%20%7C%20Windows%2011%20x64-0078D6.svg)](https://learn.microsoft.com/windows/)
[![Development Status](https://img.shields.io/badge/Status-0.1.0--dev%20(Remediation%20Baseline)-orange.svg)](docs/STATUS.md)

---

## Important Notice: Release Status & Governance Notice

> **AUTHORITATIVE NOTICE — OWNER DECISIONS Q42:A, Q43:A, Q44:A, Q45:B, Q46:B**
>
> - **Release v1.0.0 Withdrawn (Q42:A & Q45:B)**: On 2026-10-26, the prematurely tagged GitHub release `v1.0.0` and Git tag `refs/tags/v1.0.0` were formally withdrawn and deleted following an independent audit that identified missing subsystems (Direct2D rendering, Core Audio WASAPI, Lua runtime, WinUI 3 settings), unreviewed merges, and unexecuted CI workflows.
> - **v1.0.0 Reserved (Q46:B)**: Tag `v1.0.0` is strictly reserved for the future verified production release after all 20 acceptance gates in PRD Section 18.2 pass empirical qualification.
> - **Current Development Baseline**: The codebase is at prerelease baseline `0.1.0-dev`. Public repository availability (Q43:A) does not imply production readiness.
> - **Evidence Preserved**: Complete historical metadata, installer asset SHA-256 hashes, and CI error logs are archived in `local-test-artifacts/v1.0.0_release_preservation_record.json` and documented in [docs/EVIDENCE.md](docs/EVIDENCE.md). Git commit history remains intact without force-pushing per Q45:B.
> - **Owner Approval Required (Q44:A)**: All future pull request merges into `main` require explicit, documented owner approval.

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

### Reconciled Acceptance Matrix Summary (PRD Section 18.2)

| Disposition | Count | Acceptance Gates |
|:---|:---:|:---|
| `VERIFIED` | 1 | **G03** (Shell Isolation — zero Explorer injection) |
| `PARTIALLY_VERIFIED` | 6 | **G01** (Core Lifecycle), **G02** (OS Detection), **G08** (Sensor Honesty), **G12** (Config & Backup), **G16** (License/Privacy), **G17** (Safe Mode) |
| `IMPLEMENTED_UNVERIFIED` | 2 | **G07** (Interactive Mode State Machine), **G13** (Performance Benchmark Harness) |
| `NOT_IMPLEMENTED` | 7 | **G04** (Taskbar Recovery), **G06** (Desktop Layering), **G09** (WASAPI Audio Capture), **G10** (Lua Worker), **G11** (WinUI Settings), **G14** (Accessibility), **G19** (Coexistence Yielding) |
| `NOT_STARTED` | 1 | **G05** (Experimental VM Qualification) |
| `FAILED` | 3 | **G15** (5-Channel Packaging), **G18** (Evidence & CI Ledger), **G20** (Packaging Identity / Portable Storage) |

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
