# Architecture Decision Records (ADRs)

This directory serves as the authoritative index of Architecture Decision Records (ADRs) for AetherShell, conforming to the repository layout specified in [PRD.md](../../PRD.md#17-engineering-specifications-and-architecture-contracts) (Section 17) and the precedence rules defined in [AGENTS.md](../../AGENTS.md).

## Governance & Precedence

Per `AGENTS.md`, the precedence hierarchy for architectural decisions is:
1. Explicit current owner instructions.
2. Approved owner decisions in `PRD.md` (Q01–Q46, Q09R).
3. Non-negotiable safety and release constraints.
4. Verified technical evidence and platform API contracts.
5. Architecture Decision Records (ADRs).
6. Existing codebase and tests.

---

## Foundational Architecture Decision Index

The foundational architectural decisions governing AetherShell are derived from approved owner decisions in the Product Requirements Document (PRD):

### [ADR-001] Native C++/WinRT WinUI 3 for Settings Process
- **Status**: Accepted
- **Authority**: PRD Section 7.1, Owner Decisions Q01:A, Q33:A
- **Decision**: The settings application (`aethershell-settings.exe`) is implemented in native C++/WinRT using WinUI 3 as a completely separate, on-demand process. It connects to the core runtime via authenticated Named Pipe IPC and exits with 0 MB resident RAM when closed.
- **Consequences**: Zero memory footprint when settings window is closed. Prohibits embedding web engines (WebView2/Electron) or monolithic Rust UI frameworks for the primary configuration interface.

### [ADR-002] Non-Invasive Taskbar Coordination & Safe Native Fallback
- **Status**: Accepted
- **Authority**: PRD Section 4, Owner Decisions Q02:C, Q04:A, Q21:A, Q35:B
- **Decision**: The default taskbar backend operates non-invasively (`native_noop` and external appearance styling). It yields immediately if a third-party customizer is detected. No Explorer DLL injection, unsupported hooks, or kernel drivers are permitted in stable core builds.
- **Consequences**: Extreme stability; immune to Explorer update breakage. Experimental in-process bridges remain strictly isolated to disposable VMs and separate experimental channels (G05).

### [ADR-003] Passive Direct2D Desktop Layering Behind Icons
- **Status**: Accepted
- **Authority**: PRD Section 5, Owner Decisions Q06:A, Q07:B
- **Decision**: Desktop widget surfaces are rendered using Direct2D 1.1 / DirectComposition and stacked strictly between wallpaper and desktop icons (`SHELLDLL_DefView` / `WorkerW`). Window styles enforce `WS_EX_TRANSPARENT | WS_EX_NOACTIVATE` so mouse clicks pass through to desktop icons.
- **Consequences**: Desktop icons remain 100% interactive during normal operation. Widgets remain passive unless Interactive Mode is explicitly activated.

### [ADR-004] Time-Bounded Temporary Interactive Mode
- **Status**: Accepted
- **Authority**: PRD Section 5.3, Owner Decisions Q08:A, Q09R:A, Q31:A
- **Decision**: Interactive widget manipulation is permitted only during Temporary Interactive Mode, activated explicitly via tray or configurable hotkey. Surfaces remain strictly below normal application windows (`WS_EX_TOPMOST` is prohibited). Auto-dismisses back to passive mode upon Escape, 8-second inactivity timeout, focus loss, or session lock.
- **Consequences**: Desktop icon access cannot be permanently blocked by misbehaving widgets.

### [ADR-005] On-Demand Supervised Lua 5.4 Script Host Without Sandboxing Claims
- **Status**: Accepted
- **Authority**: PRD Section 13, Owner Decisions Q10:B, Q32:B
- **Decision**: Standard bundled widgets are strictly declarative and do not require Lua. Lua 5.4 runs only in a distinct child worker process (`aethershell-script-host.exe`) spawned on-demand for explicitly trusted user scripts. Containment relies on Windows Job Objects (128 MB RAM quota), pre-execution SHA-256 digest trust, heartbeats, and crash quarantine. Lua is trusted code; process separation isolates accidental crashes, not malicious code.
- **Consequences**: Core tray process never crashes if a user script faults. No arbitrary CPU hard caps (e.g. 20%) are imposed.

### [ADR-006] Canonical LocalAppData Storage Contract
- **Status**: Accepted
- **Authority**: PRD Section 6.2, Owner Decisions Q15:A, Q38:B
- **Decision**: All packaging and portable channels resolve configuration, profiles, and state to canonical `%LOCALAPPDATA%\AetherShell`. Legacy portable redirection to `<exe_dir>\data` is eliminated.
- **Consequences**: Seamless migration between portable, MSI, and MSIX distributions without data loss or configuration fragmentation.

### [ADR-007] Virtual Machine Boundary for Disruptive Shell & GPU Testing
- **Status**: Accepted
- **Authority**: PRD Section 18.3, Owner Decision Q37:A
- **Decision**: Any test or experiment that mutates Explorer/taskbar state, window Z-order, installs hooks, forces GPU device removal, or tests game anti-cheat must be executed exclusively in disposable Windows VMs. Direct testing on daily-use physical host machines is strictly prohibited.
- **Consequences**: Protects the owner's primary working environment and daily desktop stability.

### [ADR-008] Local Capability Token IPC Authorization & Role Separation
- **Status**: Accepted
- **Authority**: PRD Section 8, Owner Decisions Q20:B, Q44:A
- **Decision**: Named Pipe IPC creates pipes with upfront fail-closed DACLs (`D:P(A;;GA;;;{UserSID})`). Out-of-process management commands require a session capability token (`session_{id}.token`). Supervised worker processes are prohibited from executing privileged management commands.
- **Consequences**: Prevents unauthenticated local processes or untrusted workers from invoking shutdown, profile reload, or mode toggles.

### [ADR-009] Version v1.0.0 Reservation & Truthful Acceptance Governance
- **Status**: Accepted
- **Authority**: Owner Decisions Q42:A, Q45:B, Q46:B
- **Decision**: Release tag `v1.0.0` was formally withdrawn and is strictly reserved for the future production release after all applicable acceptance gates (G01–G20) achieve empirical qualification and owner sign-off. Active engineering proceeds under `0.1.0-dev`. All status claims must cite passing automated tests or recorded experimental evidence.
- **Consequences**: Prevents premature release claims, enforcing an evidence-led stability baseline.
