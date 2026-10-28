# AetherShell Windows VM & Physical Hardware Qualification Plan

**Document Version:** 1.0.0-draft  
**Baseline Commit:** Post-PR #51 (`eede52ffbfad5c5df533855cd2c2cfcd4a0b7e08`)  
**Target Release Phase:** Pre-Production Qualification Phase (PRD Section 18.2 / Section 14)  
**Governance Authority:** Authoritative PRD & Owner Directives (Q01–Q46, Q37:A, Q40:A, Q42:A, Q44:A, Q46:B)

---

## 1. Executive Summary & Objective

AetherShell has achieved clean component-level unit and integration test verification across its 13 workspace crates (268 unique passing tests). However, under the strict evidentiary governance defined in **PRD Section 18.2** and **AGENTS.md**, **no gate can be certified as `VERIFIED` based solely on in-process tests or headless CI runners**.

The objective of this Qualification Plan is to establish a rigorous, repeatable, and non-destructive empirical test campaign to evaluate AetherShell across:
1. Isolated, disposable Windows Virtual Machines (VMs) for disruptive shell and fault-injection testing.
2. Dedicated physical test hardware representing distinct GPU driver tiers and multi-monitor configurations.
3. Ratified PRD Section 14.2 multi-process performance budgets with live hardware instrumentation.

Successful execution and documentation of this plan is a mandatory prerequisite before any gate may transition to `VERIFIED` and before `CORE_STABLE_CANDIDATE` or production release `v1.0.0` may be formally declared.

---

## 2. Platform & Hardware Test Matrices

### 2.1 Target Operating System Matrix (Disposable VMs)

All disruptive shell and fault-injection tests must execute in clean, disposable Hyper-V or Windows Sandbox environments. In accordance with safety constraint **Q37:A**, tests that kill or restart `explorer.exe` or simulate shell failures must **NEVER** run on physical daily-use host machines.

| VM Profile ID | Windows Edition | Version / Build | Windows App SDK / WinUI Runtime | Primary Test Objective |
|:---|:---|:---|:---|:---|
| **VM-WIN10-22H2** | Windows 10 Pro x64 | Version 22H2 (Build 19045.4500+) | WinAppSDK 1.5.x Redistributable | Legacy taskbar fallback, Direct2D 1.1 compatibility, PerMonitorV2 DPI awareness. |
| **VM-WIN11-22H2** | Windows 11 Pro x64 | Version 22H2 (Build 22621.3800+) | WinAppSDK 1.5.x Redistributable | XAML taskbar non-invasive detection, Desktop layering behind `SHELLDLL_DefView`. |
| **VM-WIN11-23H2** | Windows 11 Enterprise x64 | Version 23H2 (Build 22631.3800+) | WinAppSDK 1.5.x Redistributable | General stability baseline, Named Pipe DACL validation, session token lifecycle. |
| **VM-WIN11-24H2** | Windows 11 Pro x64 | Version 24H2 (Build 26100.1100+) | WinAppSDK 1.5.x Redistributable | Modern Windows 11 kernel, Job Object containment, UTF-8 BOM-free storage. |
| **VM-WIN11-CANARY** | Windows 11 Insider Preview | Build 26300+ | WinAppSDK 1.5.x Redistributable | Forward compatibility check, UBR detection resilience, new shell message tracking. |

### 2.2 Physical Hardware & GPU Driver Tiers

Performance budgeting, frame pacing, audio loopback capture, and GPU device-loss recovery require bare-metal execution on physical hardware platforms:

| Hardware Tier ID | Hardware Specification | GPU & Driver Architecture | Audio & Display Topology |
|:---|:---|:---|:---|
| **HW-TIER-INTEL** | Intel Core i5/i7 (11th–13th Gen), 16 GB RAM | Intel Iris Xe / UHD Graphics (WDDM 3.1, Driver 31.0+) | Realtek HD Audio (WASAPI shared mode), Single 1080p @ 60 Hz (100% DPI). |
| **HW-TIER-AMD** | AMD Ryzen 5/7 5000/7000 Series, 16 GB RAM | AMD Radeon Graphics / RX 6000+ (Adrenalin 23.12+, WDDM 3.1) | Realtek / AMD High Definition Audio, Dual Monitor: 1440p @ 144 Hz (125% DPI) + 1080p @ 60 Hz (100% DPI). |
| **HW-TIER-NVIDIA** | Intel/AMD CPU, 32 GB RAM | NVIDIA GeForce RTX 3060/4070 (Game Ready / Studio Driver 550+) | HDMI/DP Audio + USB Audio DAC, Triple Monitor: 4K @ 60 Hz (150% DPI) + Dual 1440p. |

### 2.3 Software Rasterizer & Diagnostic Fallback Tier

For headless CI environments, virtual machines lacking GPU passthrough, or driver crash recovery diagnostics, AetherShell supports Microsoft Windows Advanced Rasterization Platform (WARP):

| Fallback Tier ID | Environment Specification | Rasterizer & Driver Architecture | Test Role & Diagnostic Objective |
|:---|:---|:---|:---|
| **SW-TIER-WARP** | Reference Low-End Laptop / Disposable VM | Microsoft Basic Render Driver (WARP Direct3D 11.1 software rasterizer) | Fallback rendering validation, driver reset fallback simulation, battery power simulation without discrete GPU. |

---

## 3. Detailed Verification Procedures by Gate

### Gate G01: Core Startup, Session Management & Clean Shutdown
- **Target Environment:** VM-WIN11-24H2, HW-TIER-AMD
- **Test Procedure:**
  1. Launch `aethershell.exe` in standard user context (non-elevated).
  2. Verify singleton mutex `Local\AetherShell_Singleton_{SessionId}` prevents secondary instance startup and routes CLI commands over Named Pipe.
  3. Initiate system logoff and restart while AetherShell is running.
  4. Verify handler intercepts `WM_QUERYENDSESSION` and `WM_ENDSESSION`, executing graceful shutdown within 1.5 seconds without blocking Windows shutdown or causing `WerFault.exe` crash dialogs.
- **Pass Criteria:** Clean zero-exit code in Windows Application Event Log; no orphaned child processes (`aethershell-script-host.exe`).

### Gate G02: Windows Compatibility, Multi-Monitor & Dynamic DPI
- **Target Environment:** HW-TIER-AMD, HW-TIER-NVIDIA
- **Test Procedure:**
  1. Boot on mixed-DPI multi-monitor setup (e.g. Primary 150% DPI, Secondary 100% DPI).
  2. Move active windows across displays and toggle display scaling in Windows Settings.
  3. Verify `WM_DPICHANGED` dynamically adjusts Direct2D render targets, recalculates font metrics, and repositions overlay surfaces via `SWP_NOZORDER | SWP_NOACTIVATE`.
  4. Detach secondary monitor, observe `WM_DISPLAYCHANGE`, re-attach monitor.
- **Pass Criteria:** Zero visual tearing or blurriness; overlay bounds conform exactly to virtual desktop work areas; Direct2D render targets recreate without memory leaks.

### Gate G04 & G06: Desktop Layering, Explorer Restart & Win+D Resilience
- **Target Environment:** VM-WIN11-23H2 (Strict Disposable VM per Q37:A)
- **Test Procedure:**
  1. Launch AetherShell in passive widget display mode.
  2. Press `Win+D` (Show Desktop), minimize all windows, and perform desktop icon lasso selection across widget areas.
  3. Verify overlay stays strictly behind desktop icons (`SHELLDLL_DefView`) and does not intercept icon clicks.
  4. Simulate shell termination: `taskkill /f /im explorer.exe`.
  5. Wait 3 seconds, then restart Explorer: `Start-Process explorer.exe`.
  6. Observe dynamic handling of `TaskbarCreated` message: tray icon re-registration, desktop hierarchy re-probe, and surface restacking.
- **Pass Criteria:** Zero tray icon duplication; passive layering re-asserted behind desktop icons within 500ms; Explorer memory and handle counts remain stable.

### Gate G07: Temporary Interactive Mode Hit-Testing & Safe Dismissal
- **Target Environment:** HW-TIER-INTEL, VM-WIN11-24H2
- **Test Procedure:**
  1. Trigger Interactive Mode via global hotkey `Win+Ctrl+A` or tray menu.
  2. Verify window extended styles clear `WS_EX_TRANSPARENT | WS_EX_NOACTIVATE` while strictly omitting `WS_EX_TOPMOST`.
  3. Click interactive buttons (e.g. play/pause media, open settings). Verify spatial pointer hit-testing dispatches action.
  4. Test the four mandatory auto-dismiss triggers:
     - Press `Escape` key.
     - Click outside all active interactive regions onto the desktop wallpaper.
     - Allow 8-second inactivity timeout to expire without mouse movement.
     - Switch focus to an external application window.
- **Pass Criteria:** All triggers immediately restore `WS_EX_TRANSPARENT | WS_EX_NOACTIVATE` and restack behind desktop icons.

### Gate G08 & G09: Real WASAPI Audio Loopback & Dynamic Framerate Pacing
- **Target Environment:** HW-TIER-INTEL, HW-TIER-AMD
- **Test Procedure:**
  1. Play dynamic audio content (e.g. media player, browser stream).
  2. Verify live FFT spectrum bands update dynamically in the Direct2D presentation loop.
  3. Hot-unplug default audio output device (or switch from speakers to headphones).
  4. Verify `IMMNotificationClient::OnDefaultDeviceChanged` seamlessly migrates capture stream without application crash.
  5. Unplug AC adapter on laptop hardware to switch to battery power.
  6. Verify presentation loop immediately adapts framerate from 30/60 FPS down to 15 FPS with non-waking sampling intervals.
  7. Launch a DirectX exclusive full-screen game; verify Gaming Shield suspends presentation loop rendering.
- **Pass Criteria:** Audio capture recovers within 250ms of device migration; framerate conforms to active power state; zero audio buffer underruns or COM leaks.

### Gate G10: Lua Worker Supervision, Continuous Trust Revalidation & Fault Injection
- **Target Environment:** VM-WIN11-24H2
- **Test Procedure:**
  1. Configure an active Lua widget profile with approved SHA-256 digest.
  2. Verify worker spawns in suspended state, is assigned to 128 MB Job Object, and executes under Lua 5.4.
  3. **Fault Injection A (External Tampering):** While the worker is running, externally modify the script file on disk. Verify watchdog detects mismatch within the bounded watchdog interval (nominal 2-second tick), terminates worker subprocess fail-closed, and purges `script.*` bindings.
  4. **Fault Injection B (External Deletion):** Delete the active script file. Verify worker is terminated fail-closed within the bounded watchdog interval.
  5. **Fault Injection C (Abnormal Termination):** Kill `aethershell-script-host.exe` via Task Manager or `taskkill /f`. Verify Core detects exit, purges bindings, and increments crash counter without crashing Core.
  6. **Fault Injection D (Memory Quota):** Inject a memory leak script attempting to allocate 200 MB RAM. Verify Windows Job Object terminates child worker at 128 MB commit limit; Core supervisor catches termination and enters quarantine.
- **Pass Criteria:** Core tray process remains completely unaffected across all worker faults; no zombie processes; all bindings cleanly pruned.

### Gate G11: WinUI 3 Settings Independence & Crash Isolation
- **Target Environment:** HW-TIER-INTEL, VM-WIN11-23H2
- **Test Procedure:**
  1. Launch Settings UI from tray context menu. Verify native C++/WinUI 3 executable `aethershell-settings.exe` connects via Named Pipe IPC.
  2. Mutate configuration controls (e.g. switch theme, change audio FPS mode). Verify atomic update in Core.
  3. Close Settings window. Verify process terminates cleanly and releases all resident memory (0 MB RAM).
  4. **Fault Injection:** Re-open Settings, simulate high-throughput IPC queries, and forcefully kill `aethershell-settings.exe`.
- **Pass Criteria:** Core continues running with uninterrupted audio and desktop presentation; pipe server handles broken pipe gracefully and accepts subsequent reconnections.

### Gate G13: Empirical Multi-Process Runtime Performance Qualification
- **Target Environment:** HW-TIER-INTEL, HW-TIER-AMD, HW-TIER-NVIDIA
- **Test Procedure:**
  1. Deploy release binaries with `AETHERSHELL_APP_DATA_DIR` directed to isolated storage.
  2. Execute formal qualification benchmark:
     `aethershell.exe --benchmark-runtime minimal_idle --formal --repetitions 5 --duration 60`
  3. Execute 5-repetition 60-second evaluations for active profiles: `default_widgets`, `active_media_visualization`, `mixed_monitors`.
  4. Collect process working sets (`WorkingSetSize`), private commit (`PrivateUsage`), multi-core normalized CPU, and ETW presentation metrics.
- **Pass Criteria:** All measurements must comply with ratified PRD Section 14.2 budgets (Minimal Idle: Core WS <= 30 MiB, steady CPU <= 0.1%).

### Gate G16: License, Privacy & Network Isolation Audit
- **Target Environment:** VM-WIN11-24H2
- **Test Procedure:**
  1. Attach Wireshark / Microsoft Network Monitor to network adapter.
  2. Run AetherShell continuously for 60 minutes across audio visualization, settings changes, and widget updates.
  3. Audit captured packets for DNS queries, HTTP/HTTPS requests, or outbound telemetry packets.
  4. Verify third-party license notices (`NOTICE`, `LICENSE`) and verify compiler path remapping eliminates developer paths from binaries.
- **Pass Criteria:** Exactly zero outbound network packets generated; binary strings reveal no internal paths.

### Gate G19: Coexistence Qualification with Competing Shell Customizers
- **Target Environment:** VM-WIN11-23H2
- **Test Procedure:**
  1. Install competing shell customizers in VM: `TranslucentTB`, `StartAllBack`, and `RetroBar`.
  2. Launch competing utilities, then launch AetherShell.
  3. Verify `detect_taskbar_conflicts()` identifies competing processes and deterministically yields to `NativeNoopBackend` per PRD Q21:A.
- **Pass Criteria:** Zero process termination or injection into competing utilities; zero taskbar visual flickering or competing render loops.

---

## 4. Test Execution Governance & Evidence Artifacts

1. **Pre-Execution Check:** Every test run must record environment metadata (OS build, UBR, GPU driver version, hardware specs, Git commit SHA).
2. **Artifact Packaging:** Test logs, ETW traces (`.etl`), Wireshark captures (`.pcapng`), and benchmark ledgers (`.json`) must be archived in `docs/qualification/evidence/<test-id>/`.
3. **Impartial Audit:** All test execution reports must be audited by an independent security reviewer before updating any gate status.
4. **Owner Sign-Off:** In accordance with PRD Section 18.2, formal promotion of any gate to `VERIFIED` and declaration of `CORE_STABLE_CANDIDATE` requires explicit owner authorization.
