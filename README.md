# AetherShell

> **Unified Windows Taskbar and Desktop Widget Customization Engine**  
> Offline-first, standalone Windows desktop customization application built in native Rust with modern WinUI 3 settings.

[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-Windows%2010%20%7C%20Windows%2011%20x64-0078D6.svg)](https://learn.microsoft.com/windows/)

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

AetherShell is developed under an evidence-led iterative engineering model. Progress, test outputs, and acceptance criteria are tracked in:

- [Requirements Baseline (PRD)](PRD.md)
- [Verification & Implementation Status](docs/STATUS.md)
- [Evidence Ledger](docs/EVIDENCE.md)
- [Architecture Decision Records (ADRs)](docs/adr/)

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
