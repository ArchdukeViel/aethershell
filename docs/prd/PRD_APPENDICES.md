# AetherShell — Technical Appendices & Reference Baseline

This document provides platform documentation references, hardware baseline observations, and open technical proof-of-concept (POC) tracking for the AetherShell product requirements specification.

---

## Appendix A — Source references and platform documentation

The following authoritative platform documentation citations provide the evidentiary baseline for Windows operating system behaviors, APIs, and framework contracts cited throughout the AetherShell product specification.

| Ref | Primary documentation | Why cited |
|---|---|---|
| [S01] | [Microsoft: Windows 10 release information](https://learn.microsoft.com/en-us/windows/release-health/release-information) | 22H2 support lifecycle and servicing distinction. |
| [S02] | [Microsoft: Windows 11 release information](https://learn.microsoft.com/en-us/windows/release-health/windows11-release-information) | Windows 11 build families: 22H2 (22621), 23H2 (22631), 24H2 (26100), 25H2 (26200), 26H2 (26300), 26H1 (28000). |
| [S03] | [Microsoft: SetWinEventHook](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook) | Out-of-context events and message-loop requirements. |
| [S04] | [Microsoft: SetWindowCompositionAttribute](https://learn.microsoft.com/en-us/windows/win32/dwm/setwindowcompositionattribute) | API existence and recommendation to use DwmSetWindowAttribute for modern composition. |
| [S05] | [Microsoft: Mica material](https://learn.microsoft.com/en-us/windows/apps/design/style/mica) | App-backdrop semantics and downlevel/accessibility fallbacks. |
| [S06] | [Microsoft: Loopback Recording](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording) | Shared-mode WASAPI system playback capture and event behavior. |
| [S07] | [Microsoft: DXCore adapter enumeration](https://learn.microsoft.com/en-us/windows/win32/dxcore/dxcore-enum-adapters) | GPU/compute/NPU enumeration, not universal utilization counters. |
| [S08] | [Microsoft: SetWindowDisplayAffinity](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowdisplayaffinity) | Capture-exclusion limitations and owned top-level windows. |
| [S09] | [Microsoft: WinUI 3](https://learn.microsoft.com/en-us/windows/apps/winui) | Modern Windows UI and Windows 10 compatibility. |
| [S10] | [Microsoft: Distribute an unpackaged WinUI 3 app](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/unpackage-winui-app) | Self-contained portable deployment and package-size tradeoff. |
| [S11] | [Microsoft: MSIX containerization overview](https://learn.microsoft.com/en-us/windows/msix/msix-containerization-overview) | Package filesystem behavior, virtualization and protected installation. |
| [S12] | [Microsoft: GetDiskFreeSpaceExW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getdiskfreespaceexw) | Disk-capacity reporting, not read/write throughput. |
| [S13] | [Microsoft: Windows App SDK versioning](https://learn.microsoft.com/en-us/windows/apps/get-started/versioning-overview) | Distinguish OS build, Windows SDK and Windows App SDK versions. |
| [S14] | [Microsoft: Windows App SDK support matrix](https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/support) | Windows App SDK system requirements and servicing distinctions. |
| [S15] | [Microsoft: Distribute an unpackaged WinUI 3 app](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/unpackage-winui-app) | `WindowsAppSDKSelfContained=true` deployment; packaging considerations. |
| [S16] | [Microsoft: Understanding packaged desktop apps](https://learn.microsoft.com/en-us/windows/msix/desktop/desktop-to-uwp-behind-the-scenes) | Conditional AppData virtualization and file operation behavior; distinguish full trust from AppContainer. |
| [S17] | [Microsoft: Win32 SetWinEventHook](https://learn.microsoft.com/en-us/windows/desktop/api/winuser/nf-winuser-setwineventhook) | Event callback registered thread needs message loop; STA is not a universal `FindWindow` requirement. |

---

## Appendix B — Device hardware observations & reference baseline

The observations below record the initial physical development baseline environment. They serve as reference hardware parameters for local performance benchmarks and capability probing.

| Observation | Provenance classification | Implication / what is not proven |
|---|---|---|
| Windows 11 Pro 26H2 `26300.9711`; Explorer file version `10.0.26100.8117` | Observed via registry and `FileVersionInfo` | Exact update matters; does not validate external taskbar methods across other builds. |
| AMD Ryzen 5 5500U, AMD Radeon integrated graphics, x64, 16 GB RAM; 1080p 60 Hz SDR display | Observed CIM and display inventory | Useful **reference hardware**, not a universal performance matrix. |
| Active taskbar auto-hide; `Shell_TrayWnd` rect extending beneath bottom screen edge | Observed `SHAppBarMessage(ABM_GETSTATE)`, `FindWindow`, window rect | Need per-DPI auto-hide safe-area logic; never hardcode a static 48 px clearance. |
| `ExplorerTAP.dll` loaded into Explorer; child `DesktopWindowContentBridge` present | Observed `Get-Process.Modules` and hierarchy | Indicates a third-party customization component and child XAML composition; requires clean yielding. |
| `Progman` → `SHELLDLL_DefView` → `SysListView32`, with variable `WorkerW` presence | Observed `FindWindow`/`FindWindowEx` | Need resilient host discovery and stacking logic; no fixed window parenting assumption. |
| Virtual surround output device, plus physical audio output devices | Observed audio inventory | Validate default-endpoint change and cancellation; handle virtual device removal cleanly. |
| WinUI/Windows App SDK runtimes and Rust/Visual Studio/SDK installed | Observed toolchain enumeration | Development environment capabilities; runtime memory and footprint must be measured empirically. |

---

## Appendix C — Open technical unknowns & proof-of-concept tracking

The following proof-of-concept tracks define unresolved technical investigations and their allowed fallbacks.

| Unknown | Evidence gate | Allowed fallback |
|---|---|---|
| External taskbar clear/blur/acrylic on particular Windows builds | POC-TB-01 VM pixel and recovery test | Native taskbar (`native_noop`) |
| Floating pill geometry / child XAML brush modification | POC-TB-02/03, disposable VM | Exclude from stable v1 |
| Desktop window behind icon view without breaking wallpaper | POC-DS-01 VM multi-wallpaper tests | Disable widgets on affected configuration |
| Temporary Interactive Mode not interfering with desktop icons | POC-DS-02 focus and hit-test validation | Disable direct widget actions, use tray/hotkeys |
| Sparse compositing costs on reference integrated GPU | POC-RD-01 with valid workload baseline | Reduce effects/surface design |
| Self-contained C++/WinRT WinUI Settings bundle and process footprint | POC-UI-01 build and measurement | Tune memory/packaging, retain framework choice |
| Lua worker stability under hang/native failure | POC-LUA-01 VM fault tests | Disable/quarantine scripted widgets |
| MSI/MSIX/Store/portable shared AppData correctness | POC-PK-01 clean VM upgrade/uninstall tests | Block affected channel/release, no silent data split |
| Genuine AC/battery audio wakeups and endpoint changes | POC-AU-01 power and lifecycle trace | Lower FPS, stop capture until restored |
| Advanced GPU/NPU/thermal availability and dGPU sleep | POC-SN-01 hardware inventory | Honest missing value / optional external provider |
| Game/anti-cheat interaction and shell event flags | POC-GM-01, explicitly bounded | Suspend features, no universal guarantee |
