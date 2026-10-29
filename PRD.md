# AetherShell — Product Requirements Document

**Product:** AetherShell — Unified Windows Taskbar and Desktop Widget Customization Engine  
**Target product version:** 1.0.0  
**PRD revision:** 2026-10-29 / v1.0 (Clean Specification Baseline)  
**Status:** Approved product specification; active implementation in progress (`0.1.0-dev`)  
**Platform:** Windows 10 22H2 and Windows 11, native **x86-64 only** for v1.0.0  
**Architecture:** Standalone background core (Rust) + on-demand settings (WinUI 3/C++/WinRT) + on-demand script host (Lua 5.4)  

> **Interpretation rule:** This specification defines authoritative product behavior, architectural invariants, and acceptance criteria. Windows-specific platform interactions are governed by verified platform documentation and empirical qualification. Features are classified using explicit evidence labels to distinguish established platform APIs from implementation designs, proof-of-concept hypotheses, and aspirational performance goals. No performance claim, anti-cheat compatibility, or shell integration is considered verified without recorded empirical evidence.

## 0. Executive summary and architectural principles

### Implementation Completion Matrix

| PRD Section | Title | Implementation Status | Codebase Artifacts & Verification |
|---|---|:---:|---|
| **0** | Executive Summary & Principles | ✅ Implemented | Core architecture, 3-process model, non-invasive shell model |
| **1** | Users, Journeys & Requirements | ✅ Implemented | First-run onboarding, tray navigation, safe mode, error fallback |
| **2** | Windows Platform Contract | ✅ Implemented | Registry OS/UBR detection, PerMonitorV2 DPI awareness, capability probes |
| **3** | Standalone Runtime & IPC | ✅ Implemented | Named pipe IPC with session DACLs, auth tokens, Job Object quotas |
| **4** | Taskbar Customization Engine | ✅ Implemented | Taskbar state machine, conflict detection, NativeNoop & ExternalAppearance backends |
| **5** | Desktop Host & Stacking | ✅ Implemented | WorkerW/Progman layering, desktop icon passthrough, temporary Interactive Mode |
| **6** | Widget Catalog & Bindings | 🔶 Partially Implemented | Binding dictionary & resolution implemented; authorable skin packaging pending |
| **7** | Telemetry, Audio & Media | ✅ Implemented | Win32 telemetry samplers, WASAPI shared-loopback spectrum, power-aware polling |
| **8** | Settings Application | ✅ Implemented | C++/WinRT + WinUI 3 standalone executable, 6 navigation pages, 2-way IPC sync |
| **9** | Design System (Skins & Themes) | 🔶 Partially Implemented | 6 theme token palettes implemented; built-in declarative skin packages pending |
| **10** | Configuration & Schemas | ✅ Implemented | TOML parser, atomic replace, backup rotation; bidirectional reversible schema migrations (1.0.0 <-> 1.1.0 <-> 2.0.0), Win32 exclusive locking |
| **11** | Runtime User-Data Layout | ✅ Implemented | Canonical %LOCALAPPDATA% storage, portable layout specification, zero host leakage |
| **12** | Repository Structure & Engineering | ✅ Implemented | 13 workspace crates, CMake MSBuild WinUI project, automated test inventory |
| **13** | Security, Privacy & Non-Interference | ✅ Implemented | Unrestricted out-of-process Lua, SHA-256 digest trust verification, continuous revalidation |
| **14** | Performance & Power Requirements | 🔶 In Progress | Multi-process benchmark harness implemented; Ryzen 5 baseline recorded; optimization active |
| **15** | Gaming Shield & Coexistence | ✅ Implemented | Fullscreen detection, presentation suspension, third-party customizer yielding |
| **16** | Packaging & Distribution | ⬜ Not Started | Packaging scripts exist (MSI, MSIX, portable); Store submission and signing deferred |
| **17** | Failure-Mode Coverage | 🔶 Partially Implemented | F01-F47 scenarios defined; crash recovery and device loss implemented; VM fault injection pending |
| **18** | Testing Strategy & Acceptance Gates | 🔶 In Progress | 300 unique Cargo tests + 36 PowerShell tests (336 total passing tests); 20-gate matrix tracked (18 partially verified) |
| **19** | Roadmap & Milestones | 🔶 Updated | Phases 0–4 completed; Phase 5 (VM & packaging qualification) in progress |
| **20** | Implementation Backlog & Release Artifacts | 🔶 Updated | Completed engineering milestones tracked; remaining release criteria defined |

AetherShell is an **offline-first standalone Windows desktop customization application**, combining adaptive taskbar appearance with an extensible desktop-widget and skin engine. It must coexist with the user's desktop environment, live or static wallpaper, games, media apps, and other desktop software. It runs at normal user privilege, does not install a kernel driver or Windows service, and must not require modification of critical Windows processes for its ordinary operation.

The **normal running configuration** is a Rust (`aethershell.exe`) background application containing the tray, window-state tracking, desktop-widget engine, telemetry, audio visualization, media integration, configuration, and orchestration. **Unrestricted Lua 5.4** runs only in a distinct, explicitly trusted **on-demand child process** (`aethershell-script-host.exe`), supervised over session-bound IPC. The modern **WinUI 3 + C++/WinRT Settings** executable (`aethershell-settings.exe`) is a separate on-demand process; it exits on close. Neither Settings nor the Lua worker is a resident dependency when unused. A child process limits accidental fault propagation, **not** hostile same-user code or operating-system effects.

AetherShell adheres to five core architectural principles for Windows shell coexistence:

1. **Default:** External, capability-checked taskbar styling; no Explorer injection.
2. **Optional experimental integration:** Separate opt-in **experimental builds/distribution**, never bundled in stable binaries. Any module loaded inside Explorer must be disabled by default, explicitly consented to, signed, build allowlisted, limited, uninstallable, and recoverable. Virtual machine qualification plus dedicated recoverable-system compatibility validation are mandatory before any limited opt-in distribution; no disruptive experiments on daily-use physical host machines. The feature is *not a promised v1 release capability*.
3. **Fallback:** Stock Windows taskbar behavior; no taskbar wars or continuous reapplication.
4. **Floating geometry:** A priority research objective, **not a v1 ship blocker**. A cosmetic recreation that conflicts with desktop-only/widget constraints must not be silently substituted.
5. **Windows safety:** Never attach to `dwm.exe`, `csrss.exe`, `svchost.exe`, `audiodg.exe`, game processes, or browsers; no memory detours, kernel hooks, drivers, or privilege elevation in the base app. Explorer integration is an explicitly isolated experimental research track, not general permission to interfere with other processes.

The product combines **Windows 10 and 11 compatibility**, desktop-only widgets positioned **above any static/animated wallpaper but below desktop icons**, passive desktop widget visuals below desktop icons with **temporary Interactive Mode** invoked by tray/hotkey, six initial visual skin archetypes and six themes, native baseline plus optional advanced sensors, **audio visualization enabled by default** for media such as Spotify, x64 only, **all MSI / MSIX / Microsoft Store / WinGet / self-contained portable delivery channels**, performance budgets with measured workload profiles, **Apache-2.0** licensing, and separate architecture/engineering documents.

### 0.1 Evidence labels used throughout

| Label | Meaning | Release interpretation |
|---|---|---|
| **[DOC]** | Behavior documented by authoritative platform documentation as of research cutoff | May be used when supported on target OS and verified by tests. |
| **[DESIGN]** | AetherShell requirement or chosen engineering approach | Must be implemented and tested; not a factual claim about Windows behavior. |
| **[POC]** | API/behavior uncertain, unofficial, version-sensitive, or environment-specific | No production promise without a recorded experiment and acceptance evidence. |
| **[DEPENDENT]** | Availability depends on hardware, driver, OS, external process, deployment channel, or user permission | Expose capability and fallback in product and diagnostics. |
| **[GOAL]** | Numeric budget or UX target, not a benchmark result | Verify via reproducible performance methodology. |

### 0.2 Scope and non-goals

**In scope:** Windows taskbar effects where feasible; per-monitor taskbar state rules; desktop-layered clocks, media, hardware monitors, spectrum visualizers, optional weather, profiles, themed skins, scriptable widgets; keyboard-accessible editing and settings; packaging; backup/restore; safe mode; evidence-led compatibility testing.

**Out of scope for guaranteed v1 behavior:** a universally supported floating native Windows taskbar; guaranteed true native Mica on a system taskbar; always-on-top widgets; ARM64; a hosted widget marketplace/cloud accounts; kernel-mode telemetry; audio recording or cloud upload; universal anti-cheat certification; any claim of zero CPU/GPU, perfect crash prevention, or guaranteed avoidance of all Windows security dialogs.

---

## 1. Users, journeys, and requirements

### 1.1 Primary user types

- **Everyday customizer:** installs, enables one preset, changes taskbar transparency and clock/widget design without editing a file.
- **Media listener:** uses desktop media metadata and live audio-spectrum widgets for players including Spotify; audio capture is on by default while the corresponding visualization capability is enabled, and can be turned off instantly.
- **Power user:** combines multiple widgets, monitors, custom colors, local themes, Lua code, and per-app appearance rules.
- **Laptop/gamer:** needs low idle/battery footprint and no disruptive behavior during full-screen games or sleep.
- **Skin creator:** develops declarative packages, previews them, checks schema compatibility, and shares them as local archives without a hosted marketplace.

### 1.2 First-run journey

1. On installation, show the privacy and desktop integration summary, install-path information, and exact features enabled by default.
2. Select the `Signature Minimalist` preset. Display a safe before/after preview where possible.
3. Offer taskbar styling and per-monitor positioning. If a specific effect is unsupported, show the stock-taskbar fallback.
4. Enable local media visualization by default; visibly explain it uses WASAPI loopback of system playback, is local-only, is not saved, and can be turned off or paused.
5. Show **Start with Windows** as an explicit first-run option, suggested **on** in line with the originating PRD, but clearly user-configurable and confirmed during onboarding; never hide persistence settings.
6. Keep normal desktop widgets passive below desktop icons. Clicking media controls requires an explicitly activated, temporary **Interactive Mode**; editing layout is a separate mode.
7. Provide `Open Settings`, `Unlock Layout`, `Pause Audio Visualization`, `Safe Mode`, `Diagnostics`, and `Exit` from the tray.
8. Persist atomic, versioned configuration and create a restore point before upgrades or schema migrations.

### 1.3 UX invariants

- No unexpected takeover of the taskbar, keyboard focus, desktop icon input, full-screen games, or other app windows.
- Failure of one sensor, a widget, audio stream, taskbar engine, or Settings UI does not intentionally terminate unrelated subsystems.
- AetherShell must be fully usable without an internet connection or account.
- No bundled widget should require Lua; the on-demand script-host process launches only for explicitly trusted script packages and terminates when no trusted script is active.
- Unsupported effects and unavailable sensor values display an honest fallback, *never fabricated zeros*.

---

## 2. Windows platform and compatibility contract

### 2.1 Minimum and target releases

| Operating system | Build family | Policy | Notes |
|---|---:|---|---|
| Windows 10 22H2 x64 | 19045 | v1 minimum; required compatibility tests | Standard support ended 2025-10-14; AetherShell compatibility is not Microsoft OS security support. Windows 10 ESU or other servicing status must be stated separately. [S01] |
| Windows 11 22H2 x64 | 22621 | required legacy Win11 compatibility tier | Some visual effects unavailable; this release may be outside servicing, depending on edition. |
| Windows 11 23H2 x64 | 22631 | required compatibility tier | Edition and lifecycle differences; tested behavior matters. |
| Windows 11 24H2 x64 | 26100 | required compatibility tier | Shell visuals may differ. |
| Windows 11 25H2 x64 | 26200 | primary test tier | Treat this as its own build family. |
| Windows 11 26H2 x64 | 26300 | primary test tier | October 2026 general availability; **not** 26200. [S02] |
| Windows 11 26H1 x64 where present | 28000 | special validation tier, where applicable | Distinct hardware-focused branch; do not infer feature support from numerically higher build number alone. [S02] |
| Windows 11 21H2 | 22000 | evaluate separately; not advertised until tests | Below initial Windows 11 baseline in the originating PRD. |
| Windows ARM64 | N/A | excluded from v1 | No native or emulated ARM64 support promise. |

**[DESIGN]** OS build detection occurs at startup and again after applicable shell/session changes. Capability probes and feature allowlists take precedence over simplistic `build >= number` assumptions. Product support is based on exact OS/edition/architecture/feature evidence, with Windows 10 still covered as owner-directed even when its OS servicing status differs.

### 2.2 Confirmed Windows API facts

- **[DOC]** `SetWinEventHook(..., WINEVENT_OUTOFCONTEXT)` delivers notifications without mapping the callback into the event-producing process; the registering thread needs a message loop. Events can still be stale or need debounce. [S03]
- **[DOC]** `SetWindowCompositionAttribute` exists, but Microsoft advises using `DwmSetWindowAttribute` instead. Neither statement establishes that all desired taskbar effects work on all builds. [S04]
- **[DOC]** Mica/Mica Alt are primarily *application window backdrop* materials. Windows 10 and high-contrast/battery conditions use fallbacks; do not equate application window Mica with taskbar Mica. [S05]
- **[DOC]** WASAPI shared-mode loopback can visualize system playback, with event-driven loopback supported on the target Windows 10/11 range. Capturable protected content and endpoints may differ. [S06]
- **[DOC]** DXCore can enumerate accelerators (including some NPU classes) but enumeration does **not** imply a standardized NPU-utilization percentage or thermal source. [S07]
- **[DOC]** `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` is limited to suitable owned top-level windows and is not a complete privacy or DRM guarantee. [S08]
- **[DOC]** WinUI 3 is a modern Windows UI framework, but is not proven to be the least-memory UI framework. A self-contained unpackaged distribution bundles extra runtime files and is larger. [S09] [S10]
- **[DOC]** MSIX installations use protected package files and virtualized behavior; do not assume packaged filesystem paths behave identically to MSI/portable. [S11]

### 2.3 Feature capability model

Every Windows-facing feature declares at least:

`feature_id`, `implementation_backend`, `minimum_os`, `tested_builds`, `requires_explorer_module`, `required_permissions`, `compatible_package_channels`, `probe_method`, `known_conflicts`, `fallback`, `rollback`, `last_verified_on`, `risk_tier`.

Status enum: `supported_tested`, `supported_conditional`, `experimental_opt_in`, `not_supported`, `blocked_for_safety`, `temporarily_failed`.

**Do not** silently translate a `not_supported` feature into a misleading application-wide success status.

---

## 3. Standalone runtime, process boundaries, and IPC

### 3.1 Processes and optional modules

```text
Windows interactive user session (standard user token)
|
+-- aethershell.exe                        [required while running; Rust core]
|   +-- session-owned tray and non-invasive shell events
|   +-- taskbar state + external/no-op capability coordinator
|   +-- desktop host + DirectComposition/Direct2D renderer
|   +-- profiles/themes/config + skin/package validation
|   +-- telemetry + WASAPI + media session management
|   +-- IPC servers + worker supervisor + recovery + diagnostics
|
+-- aethershell-settings.exe               [WinUI 3/C++/WinRT, on demand]
|   +-- accessible Settings, theme/skin preview and layout editing
|   +-- authenticated, versioned local IPC to core; exits when closed
|
+-- aethershell-script-host.exe            [Rust executable embedding Lua 5.4]
|   +-- launched only for approved script-bearing packages
|   +-- unrestricted Lua standard libraries and explicit native modules
|   +-- same-user privileges: NOT a sandbox or malware boundary
|   +-- lifecycle/heartbeat, structured messages, independent termination
|
+-- explorer.exe                            [Windows-owned]
    +-- NO AetherShell extension in stable distribution
    +-- separately distributed experimental Explorer component [POC]
```

**[DESIGN]** Standalone means no mandatory service, driver, cloud daemon, Explorer extension or elevation in everyday operation; multiple cooperating **user-mode processes** are permitted. Normal stable distribution must not contain the optional Explorer extension. The Settings program and script worker are individually optional/lazy and must not run merely because Windows started the core. Separate binaries must be signed and version-matched with the core.

**Lua worker contract:** authenticated/ACL-protected current-session IPC; protocol negotiation; process-instance nonce; one outstanding request cap per worker where appropriate; typed, strictly size-bounded messages (e.g. validated scene/text/metric updates, no raw pointers or COM handles); cancellation for cooperative work; bounded timeouts; heartbeat liveness; kill/restart/quarantine after repeated failure; job-object lifecycle tracking where feasible. The worker may ignore cooperative cancellation, so termination is an OS process operation. A script can deliberately kill/relaunch the parent, alter user data or launch another process; this remains an explicitly accepted security risk, not a promise that the parent is invulnerable. Prevent child-only freezes from holding UI locks or core shell-restoration work.

### 3.2 Local IPC design

- Windows named pipes (separate role-specific endpoints for Settings and Lua worker) under the current logon session; use Windows security descriptors, remote-client rejection, session identity and a per-instance nonce. Never assume a pipe name alone authenticates a peer.
- A versioned request/response envelope: `protocol_version`, `message_id`, `command`, `payload`, `deadline_ms` (and error/status response).
- Settings requests are serialized and validated by core; Settings never directly manipulates Explorer or GPU scene state.
- Commands: `get_status`, `get_capabilities`, `list_widgets`, `stage_config`, `preview_theme`, `commit_config`, `restore_config`, `set_profile`, `edit_layout`, `enter_interactive_mode`, `exit_interactive_mode`, `set_audio_capture`, `open_diagnostics`, `shutdown`. Lua-worker IPC must use an independent, more restrictive command allowlist than Settings.
- Hard size and timing limits; no arbitrary file paths executed through IPC; path validation and account-bound authorization.
- If core is not running, Settings offers startup/recovery or an offline configuration editor; no deadlock on reconnect.
- Single-instance behavior scoped to **interactive user session**, not global machine. Handle second launches by requesting existing instance to open Settings; account for Fast User Switching and Remote Desktop.
- Do not expose localhost HTTP services by default.

### 3.3 Resource and lifecycle contract

- Native event-driven message loop, worker tasks bounded and cancellable, no busy reapplication of failing shell effects.
- Separate priority classes for low-frequency metrics, presentation, and optional media processing. No guarantee that EcoQoS directly pins work to E-cores.
- Settings process starts only after an explicit user action; closing it closes IPC, releases Settings-owned windows/resources, and terminates the process.
- Internal modules use typed interfaces and structured error handling. One failing subsystem must be disabled or degraded without terminating healthy subsystems where possible.
- On session lock, monitor off, standby, resume, Explorer restart, GPU device removal/reset, and user switch, apply well-defined state transitions.
- Crash dumps and diagnostics default to local-only, size-bounded and privacy-redacted.

### 3.4 Failure policy and recovery precedence

`safe_mode` > `system_accessibility` > `fullscreen_gaming_shield` > `manual_user_disable` > `power_saving` > `per-app_rule` > `window_state` > `default_theme`.

Critical taskbar restoration takes priority over applying desired effects. Unexpected termination or hang of Lua worker must not require Explorer or AetherShell core restart. Worker crash-loop counters must disable its script package; heartbeat loss must not block the tray message loop. In a suspected Explorer crash loop, stop the optional module and clear the related startup flag. A crash of `aethershell.exe` alone cannot be guaranteed to run cleanup; startup recovery, an idempotent restoration strategy, and defensive experimentation are required.

---

## 4. Taskbar customization engine

### 4.1 User capabilities

| ID | Requirement | Fallback / gating |
|---|---|---|
| TB-01 | Discover primary and secondary taskbar windows and monitor mapping. | If discovery fails, leave native taskbar unchanged. |
| TB-02 | Per-monitor desktop/maximized/flyout/fullscreen/battery/app-rule states. | Unknown state -> conservative native/opaque behavior. |
| TB-03 | User presets: native, clear, tint, blur-like, acrylic-like, solid, conditional Mica-style where feasible. | Capability probe and visual verification; do not claim native system Mica without proof. |
| TB-04 | Accent-color customization, opacity, transitions and priorities. | Respect high-contrast, Windows transparency disabled, battery saver. |
| TB-05 | Auto-hide, taskbar on secondary monitor, mixed-DPI, alignment and work-area changes. | No geometry mutation until tested. |
| TB-06 | Floating pill geometry and native border removal as prioritized experimental research. | Not mandatory for stable v1 for stable v1 release. |
| TB-07 | Per-app, full-screen, and flyout rules without game-process injection. | Only shell-visible state and minimal process identity reads where necessary. |
| TB-08 | Recover from taskbar recreation, Explorer restart and Windows theme changes. | Debounced rebind, bounded retry; disable on repeated failure. |
| TB-09 | Detect contention with other software and yield rather than repeatedly reapply. | Warn user and switch to native taskbar. |
| TB-10 | Allow manual disable, reset-to-stock and Safe Mode from tray and CLI. | Always available from stable core. |

### 4.2 Backends

**Backend A — `native_noop` [DOC/DESIGN]**: Do not mutate the system taskbar. Preserve non-invasive state tracking only where needed. Universal fallback and automatic response to a known conflicting taskbar customization process. Detection must be conservative; do not enumerate privileged memory or attempt to control third-party processes. Offer a non-blocking explanation and manual choice to leave customization disabled.

**Backend B — `external_appearance` [POC]**: Runs only in AetherShell's own process using a per-build, per-capability allowlist. A read-only report from Windows 11 Pro 26H2 `26300.9711` reported `Shell_TrayWnd` hosting `Windows.UI.Composition.DesktopWindowContentBridge` and `ExplorerTAP.dll` loaded in the other customizer's Explorer instance. **These observations establish neither successful nor impossible AetherShell external transparency.** Do not assume success based on window style or API return code. Validate pixel-visible appearance and Explorer stability in a disposable VM. Unverified effects must resolve to `native_noop`, never repeatedly force a failing effect. `DwmSetWindowAttribute` is not a guaranteed child-XAML-brush editor; Microsoft discourages `SetWindowCompositionAttribute` for new uses. No `SetWindowRgn` shell clipping without separately approved controlled evidence.

**Backend C — `explorer_xaml_experimental` [POC]**: Not part of stable v1. The owner permits a **separately distributed opt-in experimental build** after disposable-VM qualification, a dedicated recoverable test-system compatibility stage that respects VM isolation boundaries, code signing, compatibility allowlisting, security and crash-loop recovery, and release-specific consent. Never claim Microsoft endorses arbitrary XAML tree injection. A packaged Store edition must never include the experimental module unless independently qualified and accepted under the relevant channel's rules. No module may attach to unrelated protected processes.

**Conflict rule [DESIGN]:** If a known competing taskbar customizer is active, yield and explain; its DLL being present does not authorize removal, alteration, or termination of that app. Only an explicitly selected user configuration and later compatibility evidence can permit coexistence; there is no "last writer wins" loop.

### 4.3 Deterministic state machine

Each monitor has: `Disconnected`, `Desktop`, `WindowMaximized`, `FlyoutVisible`, `Fullscreen`, `BatterySaver`, `Suspended`, `ExplorerUnavailable`, `SafeMode`.

Inputs include shell/window events, `WM_DISPLAYCHANGE`, taskbar recreation, theme/DPI changes, power signals, Windows session messages, and timed *debounce* where necessary. Out-of-context notifications are asynchronous; no microsecond instant-cut guarantee. Process windows are filtered for cloaking, tool windows, owned windows, off-screen windows, and shell UI. Unknown fullscreen states favor avoiding overlay changes.

Rules are a precedence-ordered table, not hard-coded nested `if` statements. The resolved rule stores `reason`, `effect_requested`, `effect_applied`, `backend`, and `fallback_reason`, visible in Diagnostics. Avoid polling multiple times per frame when no state changed.

### 4.4 Native floating geometry research gate

Deliver an isolated POC report with screenshots, before/after pixel evidence, focus/hit-test behavior, auto-hide behavior, taskbar buttons and tray accessibility, multi-monitor handling, Win+D, Explorer restart, cumulative-update survival, reversible cleanup, and comparisons across supported Windows builds. Where feasible without instability, retain implementation behind explicit experimental opt-in; otherwise keep normal taskbar for stable release. Do **not** substitute a duplicated taskbar with misleading controls.

---

## 5. Desktop host, wallpaper stacking, and widget interaction

### 5.1 Required stack order

```text
Top:       Ordinary application windows and full-screen applications
           Windows desktop icons / Explorer desktop icon view
           AetherShell desktop widget visuals + allowed interactive regions
Bottom:    Any wallpaper: static wallpaper, animated/live wallpaper providers
```

**[DESIGN/POC]** This enforces *desktop-only widgets, between the wallpaper and desktop icons*. AetherShell must never replace the wallpaper, take ownership of third-party wallpaper processes, send undocumented wallpaper-spawn messages without compatibility gating, or fight for topmost order. Windows does not expose a universally stable public API that guarantees this exact stacking order with all wallpaper engines. A compatibility adapter and test evidence are mandatory. If the stack cannot be constructed safely on a machine, widgets are disabled on that monitor with an explanation rather than moved above normal apps or obscuring desktop icons.

### 5.2 Desktop host strategy

- `desktop_host` abstraction: discover Explorer desktop shell/`Progman`/`WorkerW` relationships, verify target composition and desktop icon stacking, attach/rebind conservatively.
- Prohibit treating `SetWindowLongPtr(..., GWLP_HWNDPARENT, ...)` as guaranteed cross-version desktop layering. Record actual behavior with *every* supported Windows build and wallpaper coexistence target.
- Preserve desktop icon clicking, drag-select, rename, keyboard navigation, context menus, and desktop notifications.
- Preserve proper behavior after Win+D, Win+Tab, virtual desktop switching, taskbar auto-hide, display sleep, HDR toggle, resolution/DPI change, shell restart, and monitor hotplug.
- AetherShell may render separate lightweight surfaces for each monitor; avoid a full-screen redirection bitmap when practical, but measure memory rather than claiming it is zero.
- Multi-monitor identity: use durable display device path plus EDID metadata as hints, detect collisions/cloned displays; map primary fallback when monitor identity changes; prevent off-screen coordinates.
- Do not assume live wallpaper must be visually or programmatically known: detection is optional; maintaining stack order is the contract.

### 5.3 Interaction states

1. **Locked/passive [default]:** The desktop widget visual surface is below `SHELLDLL_DefView` and never captures pointer input through the desktop icon layer. Desktop icon clicks, selection rectangles, context menus and drag operations behave exactly as without AetherShell.
2. **Temporary Interactive Mode [user initiated]:** Tray command or user-configured hotkey temporarily activates an AetherShell-owned, **narrowly bounded topmost-to-desktop interaction surface** for approved media buttons or widget actions. It must not draw above unrelated normal application windows, game overlays, UAC/secure desktop, or desktop system controls. The session is visibly indicated and dismisses by Escape, explicit close, focus loss, timeout (user-configurable), session lock, full-screen Gaming Shield, or app shutdown. Controls must return input ownership immediately at exit; no background global mouse interception. Both pointer and keyboard use are testable.
3. **Edit Layout Mode:** Separate from Interactive Mode. From tray/Settings, show widget outlines and handles; keyboard nudges (1/10 DIP), snap guides, overlap prevention, undo, reset-to-position, monitor assignment and Escape-to-cancel. Changes are staged and committed atomically on confirmation.

**[POC]** A read-only host report found `Progman` → `SHELLDLL_DefView` → `SysListView32`, but a selected `WorkerW` query returned **null**. Do not hardcode that WorkerW always exists or that a fixed parent/owner transformation is safe. Test desktop layering + temporary interaction windows in disposable VMs in disposable test virtual machines; if the required depth cannot be established safely, disable that monitor's widget layer with an explanation. The report's hierarchy observations are NOT proof of successful widget placement or hit-testing.

### 5.4 Geometry and layout rules

Use **device-independent pixels (DIPs)** as the persisted layout unit, an anchor (`top_left`, `top_center`, `top_right`, `center`, `bottom_left`, `bottom_center`, `bottom_right`) plus offset, a stable monitor selector, and scale. Reflow within usable monitor bounds and respect taskbar/work-area position; guard against a widget extending under icons unintentionally, though overlapping icon-free space can be user-controlled. Position changes are transactional; previews do not overwrite stored layouts. Allow 50–200% default scale; configurable min/max per skin.

---

## 6. Widget catalog, data binding and lifecycle

### 6.1 Built-in functional widgets

| Widget type / canonical ID | Default | Data source | Update policy |
|---|---|---|---|
| Typographic clock `clock.date` | On | System time/localization | Second or minute boundary |
| System telemetry `system.metrics` | On | Standard Windows metrics | 1s default / slower on battery |
| Media card `media.session` | On, auto-hide | Windows media sessions | Events, metadata updates |
| Audio spectrum `audio.spectrum` | On, auto-hide | WASAPI playback loopback | Responsive when audio present; frame cap |
| Top status strip `status.pill` | Available | Time / optional metrics | Event + low-frequency |
| Weather `weather.current` | Available, off | User-configured provider / network | Cache and strict request budget |
| Network graph `network.throughput` | Available | OS counters with deltas | 1–5s |
| Storage/battery `system.resources` | Available | OS capacity and battery APIs | 10–60s |

The **six skin archetypes** in Section 9 are *visual families*, separate from these functional widget types. A skin can style multiple types where its layout supports the widget's bindings.

### 6.2 Observable widget lifecycle

`discovered -> validated -> available -> enabled -> visible -> suspended/hidden -> disabled/error`. States carry a reason, error code, timestamp, and reactivation policy. No zero rendering loop while hidden; pause FFT presentation if visualizer hidden; no raw audio persistence. Disabled widgets do not subscribe to sensors or network providers. A single widget failure must not invalidate unrelated widget layouts.

### 6.3 Bindings and unavailable values

A binding resolves to a typed value and optional unit, timestamp, source name, quality, and state:

`ok | warmup | temporarily_unavailable | unsupported | permission_denied | sensor_asleep | error`.

A GPU temperature widget must display `—` / `Unavailable` and accessible reason when there is no source; **never substitute `0 °C`**. Stale data must show a stale indicator after its TTL expires. Numeric formatting uses invariant schema syntax but user locale for display.

### 6.4 Declarative primitives

Required built-ins: `container`, `row`, `column`, `grid`, `text`, `icon`, `image`, `line`, `bar`, `progress`, `arc`, `sparkline`, `spectrum`, `conditional`, `spacer`, `button` (hit-tested only when supported), `clip`, `transform` and `gradient`. Limit nested depth, number of nodes, atlas size and surfaces by policy with useful validation errors; **these declarative limits do not limit unrestricted Lua execution** selected by the owner.

Bindings may reference a small documented metric namespace. Theme token references are allowed; arbitrary executable expressions are prohibited in declarative layouts. For custom computation, an explicitly trusted package may use Lua.

---

## 7. Telemetry, sensors, media and audio

### 7.1 Native basic telemetry

| Metric | Primary design | Accuracy / risk |
|---|---|---|
| CPU total / per core | Windows performance counters or correctly delta-sampled NT counters | Validate against Task Manager and known loads. |
| Physical RAM total/used | `GlobalMemoryStatusEx` | Use consistent definition of available vs used. |
| Network upload/download | `GetIfTable2` or equivalent counter snapshots | Calculate deltas/time; handle rollover and adapter resets. |
| Disk free/total | `GetDiskFreeSpaceExW` | **Capacity only**, not throughput. [S12] |
| Disk read/write throughput | Separate supported performance/ETW/PDH counter source | Optional if safe source cannot meet costs. |
| Battery charge/power mode | Windows power APIs when supported | No battery on desktop -> unavailable. |
| GPU inventory | DXGI/DXCore | Enumeration supported on target API versions; utilization conditional. |
| GPU/NPU load | Tested OS/driver/vendor counter provider | No universal default; opt-in provider interface. |
| Temperatures/fans/clocks | Optional safe read-only external/vendor providers | No bundled driver and no forced dGPU wake. |

### 7.2 Hardware-power protections

- Avoid polling sleeping discrete GPUs for advanced telemetry unless a tested, non-waking method proves safe; prefer OS-visible adapter status and user-configurable sensor priority.
- Provider contracts report `may_wake_device`, `sampling_cost`, `last_update`, and `supported_hardware`.
- If low-power state cannot be determined with sufficient confidence, skip the questionable vendor call on battery.
- Read-only integrations must be best-effort and cannot write vendor settings; never require administrator privileges for telemetry.
- All vendor SDKs and shared-memory readers are optional and dynamically discovered; do not claim LibreHardwareMonitor and HWiNFO expose identical shared-memory contracts.

### 7.3 Audio visualizer (enabled by default)

- Use WASAPI **shared-mode loopback** on the user's selected/default render endpoint. The bundled `audio.spectrum` and `media.session` widgets begin enabled, auto-hide on inactivity, and audio samples are processed in memory only. The default active presentation cap is **30 FPS on AC Balanced**, automatically **15 FPS on battery/Low Power**, and **60 FPS for Performance** where selected; a user override is possible with clear power impact feedback.
- Show an understandable first-run notice and persistent `Pause audio visualization` toggle; remember the user's change. When disabled, release the capture client and related subscriptions.
- Process amplitude/FFT output locally; no audio storage, file recordings, remote telemetry, or network publication.
- Handle mono and multichannel formats; validate format negotiation and sample conversion before FFT. Use bounded ring buffers, cancellation events, timeout-based shutdown, and endpoint notifications; **never wait indefinitely on only the audio event when shutdown must be possible**.
- Default: 64-band visual spectrum where supported, user-selectable 32/64/128 bands and 15/30/60 FPS rendering modes. FFT window size and hop size are design parameters, not fixed performance guarantees.
- Pause/stop threshold and auto-hide should be adjustable. When audio is silent, skip presentation without blocking an audio/control-thread cancellation path. Audio event/metadata state may not exactly match every music player; fallback gracefully. Preserve default-endpoint follow mode, including virtual audio endpoint loss (e.g. Razer virtual devices) and hardware replacement.
- On endpoint removal, Bluetooth switch, sleep, session lock, protected playback, driver restart or silent data, show a proper state and resume when ready.
- Do not require Spotify integration credentials or launch Spotify to operate; read Windows system media sessions when available.

### 7.4 Media sessions

Read track, artist, playback state and artwork asynchronously from the system media-session interface where available. Media controls are conditional on session capability; a player that does not expose next/previous should hide or disable those controls. Cover artwork must be decoded with size limits and discarded when replaced. Session absence -> auto-hide or neutral empty state; never invent metadata.

### 7.5 Network-based widgets

Weather and other web-enabled widgets are **disabled by default**. An explicitly trusted Lua script may perform unrestricted network actions via standard Lua libraries if available—this is deliberate owner choice and is not a security sandbox. Built-in declarative network bindings, however, must use a managed client with opt-in, host policy, DNS/redirect verification, HTTPS defaults, timeout, response-size cap, cache TTL, rate limiting, and no secret leakage. An installed skin cannot silently gain network permissions through declarative schema alone.

---

## 8. Settings application and interaction design

### 8.1 Technology decision

**[DESIGN]** The settings executable uses **WinUI 3 + C++/WinRT**, launched on demand from the Rust core, via a versioned local IPC contract. Use the Windows App SDK for UI controls and Fluent visual language. WinUI 3 is a modern native framework, **not inherently the smallest possible memory footprint**. An always-resident settings window is prohibited, but framework caches outside AetherShell's own process are not promised to vanish instantly. [S09] [S10]

Support an explicit Settings launch by tray, command-line flag (`--settings`), Windows Start Menu, and second-process activation. Require testable behavior on Windows 10 22H2 and the Windows 11 matrix; Mica/Acrylic decorations must degrade to solid colors where unsupported or disallowed.

### 8.2 Navigation map

| Page | Purpose | Essential controls |
|---|---|---|
| **Home** | Running state and quick actions | Active scene, compatibility status, audio toggle, safe mode, undo last appearance change. |
| **Taskbar** | Native and enhanced appearance | Engine, appearance states, per-monitor overrides, rules, active fallback, experimental feature warnings. |
| **Widgets** | Enable, reorder and configure widgets | Library, monitor assignment, anchor, scale, temporary Interactive Mode, auto-hide, separate edit mode. |
| **Skins** | Choose/import/local preview | Skin-type compatibility, preview, version details, source/trust warnings, validation feedback. |
| **Themes** | Semantic palette design | Six themes, light/dark variants, contrast preview, accent gradients, typography, restore defaults. |
| **Profiles** | Save/load sets of settings | Named scenes, duplicate, export/import, safe migration, restore point. |
| **Audio & Media** | Visualization behavior | Capture status, endpoint, FPS and band options, animation, media controls. |
| **Sensors** | Hardware availability | Provider status, refresh rates, dGPU wake warnings, data source and stale/unsupported indication. |
| **Gaming & Power** | Reduce interference | Per-monitor Gaming Shield, power profiles, screen-off, game rule exclusion list. |
| **Privacy & Safety** | Local permissions and recovery | Network permissions, separate-process unrestricted Lua trust warnings, worker status/quarantine, crash report export, safe mode, reset. |
| **Diagnostics** | Inspection and support | OS build, renderer, GPU, widget frame times, taskbar capability probes, logs, failure reasons. |
| **About & Updates** | Distribution information | Version, channel, licenses, check-for-update action, update/rollback notes. |

### 8.3 User experience policies

- All settings apply atomically only after schema validation; live preview is cancellable and has a reset timer for potentially dangerous taskbar changes.
- On a detected loss of Explorer stability, change the taskbar engine to `native_noop` and show a nonblocking notification; the app must not keep reapplying a failing change.
- Provide a command palette/search for extensive settings where reasonable; all essential functionality has visible navigation.
- Use native accessibility patterns: UI Automation exposure, keyboard traversal, visible focus, high contrast, text scaling, screen-reader labels, and reduced-motion settings.
- Settings process crash: core continues unaffected; the next Settings launch receives a recoverable error and current state. An unresponsive Settings instance can be terminated independently without ending the core.
- Settings launches in the user session and must not demand administrator privileges for everyday changes. Package-specific elevation prompts must be limited to the installer requirements where unavoidable.

---

## 9. Design system: widget skins, themes and visual language

### 9.1 Separation of concerns

| Layer | Examples | Owns |
|---|---|---|
| **Widget type** | `clock.date`, `system.metrics`, `audio.spectrum` | Data contract, available actions, lifecycle. |
| **Skin** | `minimal_type`, `glass_bento`, `cyber_hud` | Geometry, node hierarchy, typographic layout, interaction hit regions. |
| **Theme** | `obsidian`, `arctic`, `aurora` | Semantic colors, typography scale, corner radii, shadows, motion tokens. |
| **Profile / scene** | `signature_minimal`, `work_focus`, `music` | Enabled widget instances, skin+theme choices, per-monitor placements, taskbar rules. |

A skin may support multiple widget types, but must declare those compatible types. A widget's `id` is its *instance ID*; its `type` and `skin` can change independently. No package is entitled to override another package's identity.

### 9.2 Six launch skin archetypes

| Skin ID | Aesthetic | Intended components | Visual rules | Motion |
|---|---|---|---|---|
| `minimal_type` | Editorial / typography | Clock, date, minimal system values | No default background, large numeral scale, high whitespace | Static or subtle fades. |
| `glass_bento` | Fluent / soft-glass cards | Telemetry, media, weather | Round cards, soft border, distinct content layers | Subtle bounded transitions. |
| `cyber_hud` | Technical / neon instrumentation | Advanced CPU/GPU/NPU, network, spectrum | Angular panels, high contrast markers, lean strokes | Optional capped light/glow. |
| `terminal_mono` | Monospaced information | Uptime, network, storage, system resources | Dense restrained text, graph ticks, spacing grid | Minimal; no flicker. |
| `ambient_soft` | Calm / diffuse pastel | Clock, calendar, weather, media | Soft surfaces, muted palette, broad spacing | Slow opacity-only transitions. |
| `spectrum_dynamic` | Music / audio-reactive | Spectrum, waveform, media card | Logarithmic bars, peaks, gradients, adaptable card size | 15/30/60 FPS; inactive => no redraw. |

**Bundled examples** must include accessible contrasts and fallbacks for high contrast, reduced motion, and unsupported blur/glow features. The default profile can mix skins rather than requiring one skin family across every widget. Visual references are inspiration only; all shipped artwork, layout templates and custom assets are original or licensed.

### 9.3 Six first-party themes (proposed authored tokens)

All color values below are ordinary **unpremultiplied sRGB `#RRGGBB`**, not GPU pixel-buffer values. Opacity is a separate 0–1 float; rendering implementations convert to premultiplied BGRA only at the appropriate surface boundary.

| Theme ID | Surface | Raised surface | Primary text | Muted text | Primary accent | Secondary accent | Border |
|---|---|---|---|---|---|---|---|
| `obsidian` **(default)** | `#090B10` | `#1C2330` | `#F3F6FC` | `#A9B5C7` | `#A78BFA` | `#6EE7B7` | `#354257` |
| `arctic` | `#182233` | `#2B3D53` | `#F1F5F9` | `#B4C7D8` | `#38BDF8` | `#88D0E5` | `#4E647B` |
| `aurora` | `#111827` | `#28354A` | `#F7F7FF` | `#ABB9D1` | `#6EE7B7` | `#C4B5FD` | `#43566E` |
| `solar` | `#FAF7F0` | `#E8DED0` | `#302B28` | `#675C52` | `#A45C2F` | `#56767B` | `#C6BAAC` |
| `neon_grid` | `#080D18` | `#17263C` | `#F1FAFF` | `#ADC0D2` | `#00D9EE` | `#FF4FD8` | `#376B83` |
| `monochrome` | `#121212` | `#333333` | `#FFFFFF` | `#CBCBCB` | `#E4E4E4` | `#A8A8A8` | `#737373` |

The table supplies design inputs, **not a verified WCAG audit**. Final foreground/background combinations, warning states, disabled controls, graphics, and varying alpha must pass accessibility testing. Never put low-opacity small text over an uncontrolled live-wallpaper region without contrast compensation.

### 9.4 Semantic token hierarchy

- Colors: `surface.bg`, `surface.raised`, `surface.overlay`, `surface.border`, `text.primary`, `text.secondary`, `text.disabled`, `accent.primary`, `accent.secondary`, `status.success`, `status.warning`, `status.danger`, `chart.track`, `chart.grid`.
- Typography: `font.family`, `font.mono_family`, `font.size.{xs,sm,base,lg,xl,display}`, `font.weight.{regular,medium,bold}`, `font.line_height`, `font.letter_spacing`.
- Layout: `spacing.{1,2,3,4,6,8}`, `radius.{none,sm,md,lg,pill}`, `border.width`, `shadow.level`, `widget.padding`.
- Motion: `motion.fast_ms`, `motion.normal_ms`, `motion.slow_ms`, `motion.reduced`, `animation.max_fps`.
- Effect: `glass.opacity`, `blur.requested_radius`, `glow.intensity`, `overlay.tint` with fallback tokens.
- Overrides: `light`, `dark`, `high_contrast`, `reduced_motion`, `battery_saver` and `transparency_disabled`.

**Validation rule:** Unknown tokens, wrong types, out-of-range alpha, out-of-bounds geometry and unsupported node kinds fail with a precise JSON Pointer/TOML path. Tokens refer to semantics, not one theme's hardcoded palette. Theme files cannot contain executable expressions.

### 9.5 Default first-run profile — `signature_minimal`

- Taskbar: request `clear` on desktop and an appropriate readable material/tint on maximized states; if unsupported, use `native_noop` or a tested solid fallback.
- Clock/date at top-center using `minimal_type`, 1-second tick when seconds visible.
- CPU/RAM/GPU-or-unsupported status panel at bottom-right using `glass_bento`, adaptive 1–5 second sampling.
- `media.session` and `audio.spectrum` at bottom-left using `spectrum_dynamic`, auto-hiding when inactive, **audio on by default**; no media session -> disappear without errors.
- Respect desktop-only stack order, icon layer, monitor usable space, Windows text scale and reduced-motion setting.
- All first-run locations are suggested layout positions, not claims that they will never overlap the user's existing desktop icons; edit mode provides repositioning.

---

## 10. Configuration, themes and widget package schemas

### 10.1 Rules applying to all schemas

- Public canonical schemas must be versioned: `1.0.0` initially; backward compatibility, deprecation and migrations are documented.
- User-created files are UTF-8 without BOM where practical; schema validation reports row/column/file for TOML errors.
- Configuration and package data are distinct from executable Lua scripts. Declarative layout files are non-executable.
- `manifest.toml` is the authoritative widget/skin package metadata; `layout.toml` describes renderer nodes; `bindings.toml` resolves data and format.
- Schemas in `schemas/` are generated/maintained from authoritative shared types; CI tests example documents against JSON Schema plus cross-field custom checks. If TOML-to-JSON validation is used, document the exact mapping.
- Disallow absolute asset paths and path traversal in declarative packages; never follow unsafe package symlinks during extraction; enforce ZIP bomb, decompression, node count, and file size limits for **declarative imports**.
- **Lua is intentionally unrestricted** after explicit trust, not subject to the declarative schema restrictions or claimed as sandboxed.
- Profiles and widget instance placement are stored independently from a skin package; editing a local skin should not overwrite global profile data.

### 10.2 Master config example — `config/config.toml`

This is an **illustrative valid TOML contract**, not a compiled application configuration. Defaults are owner-approved unless described as release/first-run choices above.

```toml
schema_version = "1.0.0"
profile = "signature_minimal"

[general]
start_with_windows = true               # shown and confirmed in first-run setup
language = "system"
check_updates_manually = true
skin_library_path = ""                  # empty -> standard user skins folder
show_tray_icon = true
safe_mode = false

[taskbar]
enabled = true
engine = "external_appearance"           # native_noop | external_appearance | explorer_xaml_experimental
on_unsupported = "native_noop"
conflict_policy = "yield_and_notify"
per_monitor_rules = true
allow_experimental_explorer_bridge = false

[taskbar.states.desktop]
effect = "clear"
opacity = 0.0

[taskbar.states.maximized]
effect = "tint"                       # no assertion that native Mica works
color = "#141925"
opacity = 0.85

[taskbar.states.flyout]
effect = "solid"
color = "#171D2A"
opacity = 1.0

[taskbar.states.battery_saver]
effect = "solid"
color = "#171D2A"
opacity = 1.0

[[taskbar.app_rules]]
id = "example_editor"
executable_name = "Code.exe"
match_window_state = "maximized"
effect = "tint"
color = "#25243A"
opacity = 0.86
priority = 50

[desktop]
placement = "between_wallpaper_and_icons"
allow_always_on_top = false
allow_behind_wallpaper = false
locked = true
interaction_mode = "temporary_only"          # default widget visuals never take direct clicks
interactive_timeout_seconds = 8
snap_grid_dip = 8
autohide_taskbar_policy = "reserve_expanded_safe_area"

[audio]
visualizer_enabled = true                # requested owner default
capture_source = "default_playback_endpoint"
store_audio = false
render_fps = 30                         # AC Balanced; battery policy caps at 15 unless user overrides
spectrum_bands = 64
auto_hide_delay_seconds = 10

[telemetry]
sample_interval_ms = 1000
on_battery_sample_interval_ms = 5000
allow_optional_vendor_providers = true
avoid_waking_discrete_gpu = true

[power]
profile = "balanced"                    # low_power | balanced | performance
pause_on_session_lock = true
pause_on_monitor_off = true

[gaming]
enabled = true
mode = "per_monitor_conservative"
force_global_dormancy = false

[scripting]
enabled = true
runtime = "lua54_separate_worker"
allow_standard_libraries = true
worker_executable = "aethershell-script-host.exe"
restart_after_crash_limit = 2
require_explicit_trust_for_imported_scripts = true

[privacy]
offline_first = true
automatic_telemetry_upload = false
crash_upload = false
network_widgets_enabled_by_default = false

[updates]
channel = "stable"
auto_apply_without_consent = false
retain_rollback_release = true
```

**Invariant:** Stable distributions do not contain an Explorer module; setting `allow_experimental_explorer_bridge = true` alone must not silently attach to Explorer; an independent tested-build allowlist, signed component, consent record, and compatibility gate are required. Optional package absent => runtime says not installed.

### 10.3 Profile example — `profiles/signature_minimal.toml`

```toml
schema_version = "1.0.0"
id = "signature_minimal"
display_name = "Signature Minimal"
theme = "obsidian"

[[widgets]]
instance_id = "clock_main"
type = "clock.date"
skin = "minimal_type"
monitor = "primary"
anchor = "top_center"
offset_x_dip = 0.0
offset_y_dip = 56.0
scale = 1.0
visible = true

[[widgets]]
instance_id = "metrics_main"
type = "system.metrics"
skin = "glass_bento"
monitor = "primary"
anchor = "bottom_right"
offset_x_dip = -24.0
offset_y_dip = -68.0
scale = 1.0
visible = true

[[widgets]]
instance_id = "player_main"
type = "media.session"
skin = "spectrum_dynamic"
monitor = "primary"
anchor = "bottom_left"
offset_x_dip = 24.0
offset_y_dip = -168.0
scale = 1.0
visible = true
auto_hide_when_inactive = true

[[widgets]]
instance_id = "spectrum_main"
type = "audio.spectrum"
skin = "spectrum_dynamic"
monitor = "primary"
anchor = "bottom_left"
offset_x_dip = 24.0
offset_y_dip = -64.0
scale = 1.0
visible = true
auto_hide_when_inactive = true
```

### 10.4 Theme example — `themes/obsidian.toml`

```toml
schema_version = "1.0.0"
id = "obsidian"
label = "Obsidian"
color_space = "srgb"

[colors.surface]
bg = "#090B10"
raised = "#1C2330"
overlay = "#242E40"
border = "#354257"

[colors.text]
primary = "#F3F6FC"
secondary = "#A9B5C7"
disabled = "#8D9AAA"

[colors.accent]
primary = "#A78BFA"
secondary = "#6EE7B7"

[colors.status]
success = "#6EE7B7"
warning = "#F5BE68"
danger = "#F58F9D"

[typography]
family = "Segoe UI Variable"
mono_family = "Cascadia Mono"
scale = 1.0

[shape]
corner_radius_dip = 14.0
border_width_dip = 1.0

[motion]
fast_ms = 120
normal_ms = 220
slow_ms = 350

[effects]
surface_opacity = 0.82
glow_intensity = 0.0

[high_contrast]
use_system_colors = true

[reduced_motion]
disable_nonessential_animation = true
```

### 10.5 Widget skin package example

```text
skins/installed/glass-bento-system/
├── manifest.toml
├── layout.toml
├── bindings.toml
├── assets/
│   ├── preview.png
│   └── icon.svg
└── script.lua              # present ONLY for a trusted script package
```

`manifest.toml`:

```toml
schema_version = "1.0.0"
id = "org.aethershell.glass_bento_system"
name = "Glass Bento System"
version = "1.0.0"
author = "AetherShell"
license = "MIT"
engine_min = "1.0.0"
supported_widget_types = ["system.metrics"]
layout_file = "layout.toml"
bindings_file = "bindings.toml"
preview_image = "assets/preview.png"
script = ""                            # no Lua for this declarative example

[geometry]
width_dip = 260.0
height_dip = 96.0
min_width_dip = 180.0
max_width_dip = 600.0
min_scale = 0.5
max_scale = 2.0

[rendering]
preferred_mode = "surface"
max_fps = 5
needs_alpha = true

[interactions]
allowed = false
```

`bindings.toml`:

```toml
schema_version = "1.0.0"

[metrics.cpu]
source = "cpu.usage_percent"
unit = "percent"
fallback_text = "—"
max_age_ms = 5000

[metrics.ram]
source = "memory.used_gib"
unit = "gib"
fallback_text = "—"
max_age_ms = 5000
```

`layout.toml`:

```toml
schema_version = "1.0.0"
node_limit = 32

[[nodes]]
id = "card"
type = "container"
x = 0.0
y = 0.0
width = 260.0
height = 96.0
background = "@surface.raised"
radius = "@radius.lg"

[[nodes]]
id = "cpu"
type = "text"
parent = "card"
x = 16.0
y = 16.0
value = "CPU ${metrics.cpu:0.0}%"
color = "@text.primary"
font_size_dip = 13.0

[[nodes]]
id = "cpu_bar"
type = "progress"
parent = "card"
x = 100.0
y = 20.0
width = 140.0
height = 6.0
value_source = "metrics.cpu"
min_value = 0.0
max_value = 100.0
fill = "@accent.primary"

[[nodes]]
id = "memory"
type = "text"
parent = "card"
x = 16.0
y = 51.0
value = "RAM ${metrics.ram:0.1} GiB"
color = "@text.secondary"
font_size_dip = 13.0
```

**[DESIGN]** The final runtime validator must define grammar for `${...}` format placeholders, token names, parent references, type-specific field ranges, supported icon/vector formats, image decoding limits, and constraints on executable content. The examples establish intended structure, not a claim of an already implemented parser.

### 10.6 Schema publication and migration

- Machine-readable draft schemas: `schemas/config.schema.json`, `schemas/profile.schema.json`, `schemas/theme.schema.json`, `schemas/skin-manifest.schema.json`, `schemas/widget-layout.schema.json`, `schemas/widget-bindings.schema.json`.
- Structural validation + semantic validation + compatibility validation + trust validation, all independently testable.
- `schema_version` uses semantic versioning; reject unknown major versions with explanation; migrate known minor versions transactionally.
- Schema migration generates a backup and compatibility report, then swaps files atomically; on failure, restore previous package/configuration.
- Golden fixtures for valid/invalid TOML, path traversals, bad token cycles, invalid layout parent graphs, excessive nodes, duplicate IDs, missing bindings and unsupported API requirements.
- Package hashes may be recorded for tamper detection, but **do not confuse a checksum with publisher identity or trust**.

---

## 11. Runtime user-data layout and file placement

### 11.1 Resolved Windows paths

**[DESIGN]** Resolve known folders through Windows APIs rather than assuming environment variable strings are stable across packaging contexts. Account for MSIX virtualization before shipping. [S11]

**Canonical user-data model (MSI, MSIX, Store, and portable). Actual package identity/redirection must be verified per installation channel:**

```text
%APPDATA%\AetherShell\
├── config\
│   ├── config.toml
│   └── config.toml.bak
├── profiles\
│   ├── signature_minimal.toml
│   └── user-created-name.toml
├── themes\
│   └── imported\
├── permissions\
│   └── trusted_scripts.toml
└── backups\
    └── timestamped-config-restore-points\

%LOCALAPPDATA%\AetherShell\
├── cache\
│   ├── media-artwork\
│   └── weather\
├── logs\
├── crashes\
├── diagnostics\
├── state\
│   ├── safe-mode.json
│   ├── window-snapshot.json
│   └── display-identity-map.json
└── skins\
    ├── installed\
    ├── imported\
    └── development\
```

Portable mode is **executable-portable but AppData-backed**: it does not keep settings next to the executable by default, even when unpacked on a USB drive. If the standard AppData location is unavailable, show a read-only/unavailable-state error and allow an explicit **user-selected writable path** without silently creating a second competing configuration store. Ensure one canonical config identity, migration/merge prompts, ownership and concurrent-write prevention between installed and portable instances. The skin library path is configurable, with path selection and containment checks. Executable scripting privileges follow the explicit trust decision, not a claim that imported packages are isolated from the filesystem. Cache, log and crash directories have documented retention and size limits; profiles and user skins are never silently deleted during uninstall.

**Portable self-contained layout:**

```text
AetherShell-Portable-x64\
├── aethershell.exe
├── aethershell-settings.exe
├── runtime\                   # self-contained Windows App SDK dependencies
├── assets\
├── config\
├── profiles\
├── themes\
├── skins\
├── cache\
└── logs\
```

Portable mode avoids writing application settings into normal installed-user-data locations **where feasible**. Windows itself may still create caches, logs, security records or other OS-managed files; "portable" is not a promise of absolutely zero OS writes. Portable-mode detection must be explicit (manifest/flag) rather than based merely on executable location. If the portable folder is read-only, fail with a clear message and allow an explicit writable data directory.

### 11.2 File safety and reliability

- Never store secrets or raw audio in cleartext logs. Credentials for optional provider APIs use Windows DPAPI/Credential Manager or supported encrypted storage; the config references a credential identifier, not a token.
- Use atomic replace for config/profile state; write temporary data in the same filesystem and preserve last-known-good backup.
- Validate every config at startup and prior to live reload; malformed config -> reject update and remain on last-known-good, with precise error diagnostics.
- Never modify the original imported archive after install; unpack into an owned, versioned package directory.
- Support backup/export/import of portable-safe settings and skin manifests, excluding credentials, machine-specific secrets, crash logs and sensitive local paths by default.
- Avoid accidental concurrent writes from Settings and live-file editor through generation IDs or optimistic-concurrency version checks.
- The location/layout in MSIX and Store distributions is **conditional on real packaging validation**; virtualized writes and package identity may require a shared user-writable storage design. Do not implement by assuming a literal env var bypasses all redirects.

---

## 12. New repository structure and engineering ownership

### 12.1 Proposed root tree (canonical v1 architecture)

```text
aethershell/
├── .cargo/
│   └── config.toml
├── .github/
│   ├── CODEOWNERS
│   ├── ISSUE_TEMPLATE/
│   ├── PULL_REQUEST_TEMPLATE.md
│   └── workflows/
│       ├── ci.yml
│       ├── security.yml
│       ├── windows-matrix.yml
│       ├── packaging.yml
│       └── release.yml
├── .gitignore
├── .gitattributes
├── .editorconfig
├── Cargo.toml
├── Cargo.lock
├── rust-toolchain.toml
├── CMakePresets.json
├── AGENTS.md
├── LICENSE
├── NOTICE.md
├── README.md
├── CONTRIBUTING.md
├── SECURITY.md
├── CHANGELOG.md
├── crates/
│   ├── aethershell-app/           # Rust binary, tray, startup, command router
│   ├── aethershell-core/          # Pure state model, orchestration, event contracts
│   ├── aethershell-win/           # Windows API wrappers and OS capability probes
│   ├── aethershell-taskbar/       # Taskbar state machine + backend policy
│   ├── aethershell-desktop/       # Desktop host and stacking backend
│   ├── aethershell-interaction/   # Temporary Interactive Mode + edit mode lifecycle
│   ├── aethershell-renderer/      # DComp/D2D/DirectWrite/DXGI rendering
│   ├── aethershell-widgets/       # Widget registry, lifecycle, bindings
│   ├── aethershell-themes/        # Design tokens, palettes, skin styling
│   ├── aethershell-config/        # Typed configs, validation, migrations
│   ├── aethershell-telemetry/     # CPU/RAM/disk/network/provider interfaces
│   ├── aethershell-audio/         # WASAPI capture, cancellation and FFT
│   ├── aethershell-media/         # SMTC metadata/artwork/controls
│   ├── aethershell-script-host/   # Rust executable embedding unrestricted Lua; lazy child
│   ├── aethershell-script-protocol/ # Rust IPC models and worker state contracts
│   ├── aethershell-security/      # Import trust UX/policy, path/IPC safeguards
│   ├── aethershell-ipc/           # Shared Rust IPC protocol and client/server
│   └── aethershell-diagnostics/   # Logs, health events, benchmark counters
├── apps/
│   └── settings-winui/            # C++/WinRT + WinUI 3 (CMake/MSBuild project)
│       ├── AetherShell.Settings.slnx
│       ├── AetherShell.Settings.vcxproj
│       ├── App.xaml
│       ├── App.xaml.cpp
│       ├── MainWindow.xaml
│       ├── MainWindow.xaml.cpp
│       ├── Views/
│       ├── ViewModels/
│       ├── Services/
│       ├── Controls/
│       ├── Resources/
│       └── Tests/
├── extensions/
│   └── explorer-experimental/     # [POC] NOT in default builds or packages
│       ├── README.md
│       ├── src/
│       ├── compatibility/
│       └── tests/
├── assets/
│   ├── app.manifest
│   ├── icons/
│   ├── fonts/                     # only licensed/distributable fonts
│   └── images/
├── skins/
│   └── built-in/
│       ├── minimal-type/
│       ├── glass-bento/
│       ├── cyber-hud/
│       ├── terminal-mono/
│       ├── ambient-soft/
│       └── spectrum-dynamic/
├── themes/
│   └── built-in/
│       ├── obsidian.toml
│       ├── arctic.toml
│       ├── aurora.toml
│       ├── solar.toml
│       ├── neon-grid.toml
│       └── monochrome.toml
├── profiles/
│   └── built-in/
│       └── signature_minimal.toml
├── schemas/
│   ├── config.schema.json
│   ├── profile.schema.json
│   ├── theme.schema.json
│   ├── skin-manifest.schema.json
│   ├── widget-layout.schema.json
│   └── widget-bindings.schema.json
├── packaging/
│   ├── msi/                        # WiX authoring + uninstall/migration checks
│   ├── msix/                       # package identity, full-trust, StartupTask
│   ├── store/                      # Store manifests/certification artifacts
│   ├── winget/                     # versioned winget-pkgs manifests
│   ├── portable/                   # self-contained Windows App SDK bundle
│   └── signing/                    # scripts/policies; NO private keys in repo
├── tests/
│   ├── integration/
│   ├── windows-matrix/
│   ├── explorer-restart/
│   ├── wallpaper-coexistence/
│   ├── input-hit-testing/
│   ├── multi-monitor/
│   ├── audio-endpoints/
│   ├── sensors/
│   ├── failure-injection/
│   ├── configuration/
│   ├── security/
│   ├── accessibility/
│   ├── performance/
│   └── fixtures/
├── benchmarks/
│   ├── scenarios/
│   └── baselines/
├── examples/
│   ├── skins/
│   ├── themes/
│   └── scripts/                  # examples labeled TRUSTED CODE
├── tools/
│   ├── powershell/
│   ├── asset-validation/
│   ├── packaging/
│   └── benchmark-runner/
└── docs/
    ├── prd/
    │   └── AetherShell_PRD.md
    ├── architecture/
    │   ├── overview.md
    │   ├── dependency-graph.md
    │   ├── ipc-protocol.md
    │   ├── taskbar-engine.md
    │   ├── desktop-host.md
    │   └── widget-runtime.md
    ├── adr/
    ├── research/
    │   ├── windows-api-capabilities.md
    │   ├── desktop-stacking-poc.md
    │   ├── explorer-bridge-poc.md
    │   └── host-evidence-2026-10-25.md
    ├── compatibility/
    │   ├── test-matrix.md
    │   └── build-capabilities.json
    ├── security/
    │   ├── threat-model.md
    │   ├── trusted-script-warning.md
    │   └── release-checklist.md
    ├── design/
    │   ├── tokens.md
    │   └── skin-authoring.md
    ├── testing/
    │   ├── acceptance-cases.md
    │   └── benchmark-methodology.md
    ├── distribution/
    └── operations/
        ├── safe-mode.md
        ├── rollback.md
        └── incident-response.md
```

### 12.2 Repository architecture rules

- Crates are **illustrative proposed boundaries**. Do not create a separate crate unless it provides meaningful ownership, testability or compilation isolation; consolidate if prototype results justify.
- Rust `aethershell-core` must not depend on `win32`, renderer APIs, network clients, UI frameworks, or Lua. Platform adapters depend on core types, not inversely.
- `aethershell-win` owns raw Windows FFI wrappers, feature probes, thread-affinity safety contracts and unsafe-function documentation. Minimize `unsafe` and document invariants.
- `aethershell-script-host` is an independent **on-demand child executable**, with unrestricted trusted Lua under explicit trust. Its process boundary provides fault containment but is **not a sandbox** against same-user attacks. Keep the host process and Lua ABI out of the Rust core binary, and do not initialize Lua when no trusted scripted widget is active.
- `apps/settings-winui` builds without the Rust renderer; settings must be usable when all taskbar effects are disabled.
- The experimental Explorer extension is physically separate from all stable build inputs and artifacts, beyond merely relying on a runtime toggle. Keep allowlists, signatures, evidence and distribution explicitly distinct; limited opt-in experimental release only after disposable-VM qualification. It must never be linked into stable installation silently.
- Keep original source and reference documentation in Git; keep measurements, personal logs, crash dumps, generated SDK files and temporary skins out of Git unless anonymized/curated.
- Rust source and module names use `snake_case`; Cargo crates and resource-folder names use `kebab-case`; public schema IDs use stable dotted names; Windows C++ conventions must be documented and consistent.
- Dependency pins and audit reports in CI; avoid abandoned libraries, bundled unlicensed fonts, and unchecked binary assets. The root `LICENSE` **must contain the Apache License, Version 2.0**, with project copyright notice and appropriate third-party `NOTICE`/license disclosure after legal/dependency checks.
- `AGENTS.md` covers working conventions and project-specific AI contributor constraints; it must not supersede the PRD or imply AI tools possess unrestricted system rights.
- PRs need clear change scope, linked requirements/ADR, tests, measured impact if performance-sensitive, migration notes, and human review for shell or security-sensitive modifications.

### 12.3 Developer build and release pipelines

- Build native x64 Rust core + `aethershell-script-host.exe` using a pinned stable Rust toolchain; build WinUI 3 C++/WinRT Settings through a pinned Visual Studio/Windows App SDK toolchain. Validate correct Windows 10 and 11 deployment dependencies; compiled sizes and process memory are not predicted from SDK presence.
- Compile/link correctness tests for the IPC contract across Rust and C++; compatibility golden tests for schema migrations.
- CI phases: format/lint/static analysis -> unit -> package/schema validation -> Windows integration -> UI automation/accessibility -> security/license -> rendering/profile benchmarks -> signed package smoke test.
- The nightly Windows matrix runs on actual Windows versions or verified virtual machines; some desktop compositing, hardware audio, and GPU scenarios require physical machines.
- Every release records compiler/SDK versions, artifact hashes, SBOM, provenance, signing identity, and Windows build matrix evidence.

---

## 13. Security, privacy and process non-interference

### 13.1 Threat model and trust boundaries

| Boundary | Threats | Controls / accepted risks |
|---|---|---|
| Windows shell/taskbar | Crash, frozen UI, rendering conflict, wrong geometry | Default external/no-op with capability gates; debouncing, restoration and refusal to compete; optional Explorer bridge in separate experimental distribution only. |
| Own desktop windows | Icon input capture, focus theft, blocking apps | Passive below icon layer; time-bounded Interactive Mode only by explicit user action; no global hooks. |
| Skin ZIP/importer | Traversal, symlink escape, archive bombs, malformed assets | Validate/stage/extract with quotas before enabling; separate schema failures from Lua execution. |
| Trusted Lua worker (separate process) | Filesystem/network/process access, same-user attacks, unbounded children/resources, stale output, IPC abuse | Explicit hash-bound consent; worker supervision, strict typed IPC and independent termination; **NOT a security sandbox**. A malicious unrestricted script can still harm user data or affect the core by OS facilities. |
| Local IPC | Cross-session spoofing, unexpected commands, excessive payloads | Session-scoped endpoints, ACLs, client identity checking, nonce, versioning, maximum message sizes, tight command allowlists. |
| Network widgets | SSRF, data exfiltration, tracking | Built-in managed requests opt-in and guarded; unrestricted trusted Lua retains independent network abilities. |
| Logs/diagnostics | Personal metadata disclosure | Redaction, size/retention limits, opt-in export, no automatic upload. |
| Installers/update files | Tampered binary, downgrade, persistence | Signed/versioned release artifacts, hashes/SBOM, channel-aware updates, rollback. |

### 13.2 Explicit unrestricted out-of-process Lua policy

Lua 5.4 executes only inside **`aethershell-script-host.exe`**, not in the permanent Rust core. Lua scripts execute out-of-process while retaining unrestricted capabilities within the child process upon explicit user trust approval. Standard `os`, `io`, `package` libraries and native extensions may remain available. Do not claim a sandbox, hard CPU cap, mandatory instruction hooks, or automatic protection against intentional hostile behavior.

1. Built-in widgets are declarative and do not launch Lua by default; import of declarative-only packages cannot silently activate Lua.
2. Third-party script packages are disabled until a user reviews their contents/source and grants explicit trust for the **specific package digest/version**. Script-content changes revoke the grant until renewed.
3. Warning text must explicitly state: **"This script runs with your Windows user permissions and may access files, use the network, run programs, affect other apps, or damage data. A separate process reduces accidental AetherShell crashes but does not make the script safe. Only run scripts you trust."**
4. Core launches worker on demand with minimized inherited handles, a controlled working directory, robust process identity and IPC verification; worker is stopped when last scripted widget stops, on Safe Mode, and during shutdown. No privileged token or unnecessary Explorer/game process handles are passed.
5. Worker sends declarative scene/data updates over a strictly validated, bounded protocol. Invalid input is dropped; renderer and taskbar use no worker-controlled raw handles, addresses, executable commands or arbitrary DLL paths.
6. Heartbeat and process-exit detection are independent of UI/event processing. A hung worker is terminated by the core after **benchmarked** grace periods, then quarantined/restarted only within finite budgets. No guarantee that intentional script behavior cannot terminate or modify core via ordinary same-user OS rights.
7. Safe Mode disables all imported scripts before launch. Tray/Settings support "Disable scripted widgets", worker status, recent crash reason and last trusted package digest.
8. Optional future restricted worker profiles would require another explicit owner decision; do not silently impose Lua library restrictions.

**Residual risk accepted by owner:** process separation improves ordinary crash containment but is **not a confidentiality/integrity security boundary** when scripts are unrestricted under the same Windows user. Hostile or compromised packages may interfere with AetherShell or external applications. Trust review, transparent warnings and provenance are required rather than marketing claims.

### 13.3 Explorer integration security exception

- Default builds/launch modes must not inject into Explorer.
- Before any limited opt-in experimental distribution, require a **disposable-VM compatibility campaign**, then dedicated recoverable physical-hardware validation with non-invasive host access unless independently reauthorized, and explicit safety/release approval. Record shell crash loops, OS updates, anti-cheat interaction, MSIX/Store restrictions, code signing, architecture, module load/unload lifetime and cleanup. Never execute disruptive experiments on the daily-use physical host.
- The bridge must never run in elevated or protected Windows processes, bypass OS integrity protections, disable protections, or use unsupported forced techniques on prohibited builds.
- Must fail closed on version mismatch or missing compatibility records; base application remains functional.
- Dedicated experimental source, build and distribution channel prevent accidental inclusion in **any stable distribution artifact**; CI scans delivered binaries and manifests for prohibited bridge modules.
- Respect the user's desire for advanced taskbar appearance **without** falsely advertising universal compatibility.

### 13.4 Privacy defaults

- No mandatory account, cloud login, usage analytics, telemetry uploads, external dependency or auto-synchronization.
- Media metadata and captured audio remain local; the core discards audio sample buffers after visualization processing.
- Network-capable built-in widgets are off until configured; make request destinations visible.
- Optional provider credentials are encrypted using platform-supported user-bound storage, not persisted as literal configuration text.
- `WDA_EXCLUDEFROMCAPTURE` may reduce ordinary screen recording exposure where applicable but is **not a security/privacy guarantee**. [S08]
- Crash logs are local by default. A diagnostics export must preview included fields and let the user redact personal paths, window titles, usernames and media metadata.

### 13.5 Accessibility and safety requirements

- UI Automation and keyboard operation for Settings and interactive widget controls; meaningful labels for gauges and progress.
- Support Windows High Contrast, OS reduced-motion/transparency settings, minimum contrast, DPI scale and text scale; use system colors where appropriate.
- Never block Task Manager, secure desktop/UAC, system recovery keys, Start menu or Windows sign-in.
- Never install a service/driver or require admin privileges to use ordinary taskbar/widget functionality.
- Restore original appearance on normal exit where safely reversible, but explicitly document unavoidable restoration limits following OS/process crashes.

---

## 14. Performance and power requirements (measured, workload-specific)

### 14.1 Measurement methodology — mandatory

Benchmarks must record: OS edition/build/patch, CPU/GPU and driver, RAM, display resolution/Hz/HDR/DPI, number of monitors, power source/profile, background interference, app release hash, packaging channel, theme/skin, widget count, and whether Settings/Lua/audio are active. Collect 60-second steady-state runs after warm-up, with at least 5 repetitions per hardware profile; report median, p95 and worst case where relevant. Separate process working set, private bytes, system commit, GPU memory, utilization and wakeups; **never derive working-set use from a sum labeled private commit**.

### 14.2 Proposed acceptance targets (GOALS, not observed values)

| Profile | Test conditions | Preliminary targets / measurement |
|---|---|---|
| **Idle minimal** | One monitor; clock + 1Hz telemetry, no music, no Settings, no Lua | Core working set **≤ 30 MiB** measured median; steady CPU **≤ 0.1%** on named reference hardware, with variance documented; 12–15 MiB **stretch goal**, not ship guarantee. |
| **Default desktop** | One monitor; default 4 widgets, media visualizer auto-hidden without music | Working set and idle CPU bounded by empirically ratified default-profile baseline; no continuous 60 FPS redraw when media absent. |
| **Active audio** | Default preset, loopback capture, 64 bands, 30 FPS | Profile process CPU/GPU, dropped frames, device-buffer underruns and energy; no arbitrary universal 0.35% CPU or 0.40% GPU promise. |
| **Multiple monitors** | Three mixed-DPI displays, widget instances on each | Incremental memory per canvas/widget, frame latency, no unbounded monitor-count growth. |
| **Battery saver** | Laptop on DC power, low-power preset | Lower polling and presentation rates, no unnecessary dGPU wake; compare measured energy impact to no-app baseline. |
| **Gaming Shield** | Borderless or fullscreen app on one monitor | No AetherShell overlays on gaming monitor, avoid taskbar churn, record p95 frame pacing impact using repeatable workload. |
| **Settings open** | WinUI 3 separate process | Measure Settings process independently; closing terminates that process, not necessarily instant removal of all OS caches. |
| **Trusted Lua active** | Representative benign and runaway scripts | Observe and disclose potential unbounded resource impact; do **not** claim strict cap or process isolation. |
| **Monitor off/session locked** | Normal lock/sleep/resume | Rendering/capture suspended when indicated; occasional OS bookkeeping work is permissible, **not guaranteed 0.00%**. |

Initial cold launch goal: **p95 ≤ 1.5 seconds** to a usable tray/core on the defined reference desktop, with optional taskbar effects asynchronous and failure-safe. Separate time-to-visible-widgets and time-to-settings-ready measurements. Initial taskbar reaction goal: **p95 ≤ 150 ms** after a delivered state event where supported; delayed shell events and OS contention are separately classified. These numerical figures are design targets to ratify after the first POC, not fake historical measurements.

### 14.3 Power profile policy

| Property | Low Power | Balanced (default) | Performance |
|---|---|---|---|
| Basic telemetry | 5–10 s | 1 s | 0.5–1 s |
| Active spectrum max FPS | 15 | 30 | 60 |
| Optional sensor providers | Minimize/sleep-aware | Conditional | Allowed while safe |
| Nonessential effects | Disabled | Moderate | User-selected |
| Idle animations | None | Minimal | Bounded |
| dGPU wake avoidance | Strict | Strict | Explicit user override possible only after risk explanation |

High-cost features must expose actual measured impact, not simply consume a nominal "GPU budget." **Adaptive default:** on battery, use the 15 FPS Low Power spectrum cap, on AC Balanced use 30 FPS, Performance may reach 60 FPS. The user may override automatically chosen caps with a clear battery-impact indicator; system lock, monitor-off and Gaming Shield override animation preference. Stop presentation when capture is silent, but keep endpoint notifications and cancellation responsive. Dynamic refresh changes do not change system-wide power or CPU affinity settings.

### 14.4 Performance regression gates & empirical baseline ratification

- Every release compares scenario measurements against a signed versioned baseline; use tolerances designed from observed variance, not guessed machine-independent constants.
- Red flags: new periodic polling at idle, persistent redraw while hidden, increasing private bytes after repeated Settings open/close, retained swapchains after audio stops, memory growth after sleep/resume, taskbar event flood, and dGPU unexpectedly waking.
- A claimed improvement must include reproducible raw benchmark output and test-machine specification. Optimization may not regress stability, accessibility, or Windows fallback behavior.

#### Ratified Empirical Hardware Baseline (Gate G13 / AMD Ryzen 5 5500U)

Under explicit owner decision and Section 14.4 governance:
- **Platform:** Bare-metal AMD Ryzen 5 5500U, 16 GB physical RAM, AMD Radeon Graphics (WDDM 3.1 Driver 31.0.21925.1001), Windows 11 Pro 26H2 Build 26300.
- **Formal Evaluation:** 5 repetitions of 60 seconds each under isolated AppData (`docs/qualification/ryzen5_5500u_benchmark_ledger.json`, EVD-035).
- **Observed Metrics:**
  - **Private Commit:** 25.2 MB median (p95: 25.6 MB) — **PASS** (strictly within preliminary $\le$ 30 MB budget).
  - **Physical WorkingSetSize:** 43.8 MB median (p95: 44.0 MB, variance < 0.57 MB) — **RATIFIED** as the empirical multi-process baseline for the Rust + Direct2D + DirectComposition runtime on Windows 11.
  - **Normalized CPU:** 0.52% median (p95: 1.04%, max: 1.82%).
  - **Host Isolation:** 0 items written to `%LOCALAPPDATA%\AetherShell`.
- **Targeted Working Set Trimming:** Active via Win32 `SetProcessWorkingSetSize` (`trim_working_set()`) after initialization and during prolonged idle silence to shed unpaged buffers.

---

## 15. Gaming Shield, power and process coexistence

### 15.1 Required gaming behavior

- Default **per-monitor conservative mode**: detect eligible full-screen or borderless full-screen applications by visible shell/window state, not game-process injection.
- On gaming monitor: disable desktop widget visibility/presentation and suspend competing taskbar appearance changes; preserve OS focus/input behavior.
- Non-gaming monitors may continue widgets if no test evidence of interference; optional `force_global_dormancy` freezes all nonessential animated/telemetry activity.
- AetherShell does not use `WH_MOUSE_LL`, keyboard interception, memory reads of games, anti-cheat bypasses, or injected game overlays.
- There is **no universal anti-cheat compatibility guarantee**; document known limitations and only claim per-title compatibility based on test evidence.

### 15.2 Other desktop applications

- Taskbar-style conflict detection: when a known competing application is active (the host agent observed one with an Explorer-resident component), **yield and notify**; do not unload its DLL, terminate it, force ownership or repeatedly reapply. Detection is best-effort, not authority to inspect protected processes.
- Wallpaper coexistence: preserve independently running wallpaper applications; AetherShell remains above wallpaper and below icon layer (subject to POC). Do not hook, restart, terminate or configure other wallpaper processes.
- Screen recorders/streamers: streamer mode may toggle capture exclusion for owned eligible windows; explain limitations and provide manual hide shortcut.
- Accessibility/remote desktop: system-owned high-contrast and composition restrictions override requested ornamental effects; if compositor unavailable/unstable, show native behavior.

### 15.3 Session and device lifecycle

- Power messages are hints, not proof every GPU context is idle; stop rendering via own scheduler and release capture resources when safe.
- Ensure wake/resume cancels stale timer deltas to prevent giant waveform or animated-clock jumps.
- Session-lock and multi-user transitions must not leak another user's media state or user-specific IPC.
- Adapter removal/device loss triggers renderer recreation with bounded retries; repeated failure disables the renderer while tray and recovery remain available.

### 15.4 OLED care and reduced-motion handling

- Retain the originating PRD's **optional OLED-care feature**, including subtle compositor-level widget shifts (original concept: within ±4 DIP) and opacity dimming after configurable inactivity. Do not claim guaranteed burn-in prevention or zero GPU cost.
- Pixel shifting may move visual content without changing an interactive hit region only if input alignment is updated consistently. Prefer disabling pixel shift for interactive or tightly icon-adjacent widgets if hit testing would be inaccurate.
- The dim timer and motion mode are user-controllable; no dimming that defeats high-contrast visibility or required accessibility controls.
- On power saver/reduced motion/high contrast, default to static visuals unless the user makes a compatible explicit choice. Test effect transitions on HDR/OLED and mixed-DPI displays.

---

## 16. Packaging, startup, installation, updates and distribution

### 16.1 All channels are v1.0.0 launch requirements

| Channel | Package content / architecture | Requirements / caveats |
|---|---|---|
| MSI | Native x64 Rust core + C++/WinRT WinUI Settings + required runtime deployment | Per-user default where feasible; WiX packaging, start menu, uninstall, repair, upgrade/migration tests. |
| MSIX | Signed full-trust Windows desktop app, x64 | Package identity, file virtualization, valid capabilities and startup behavior; channel-appropriate permission/policy compliance. |
| Microsoft Store | Submitted eligible MSIX package | **Approval is external**; no promise of guaranteed certification. Experimental Explorer module excluded if incompatible with store policies. |
| WinGet | Manifests for eligible signed installers | Require real versioned release URL, SHA256, installer metadata, idempotent update/uninstall. |
| Portable ZIP | Native x64 core, Settings and script worker, assets and **self-contained WinUI 3 runtime** | installation-free binaries with user config in AppData; no separate Windows App SDK installer required, disk footprint measured, not assumed; update collision handling. [S10] |

Stable v1 distribution requirements are **all five channels**, not a phased reduction. **Microsoft Store certification/availability is a strict stable-v1 release gate**, as is independently qualified MSI, MSIX, WinGet and portable. If any channel cannot qualify, **do not label a release stable v1.0.0**; maintain preview/release-candidate status, record blocker and request a *new* explicit owner decision if scope is ever to change. Do not bypass Store policies. A channel may omit experimental Explorer integration while still delivering the stable application.

### 16.2 Shared packaging invariants

- Core and Settings binaries must be the same product version and IPC protocol; invalid version pairing stops Settings connection gracefully.
- The optional experimental Explorer module is **excluded from every stable package**; it must be a separate signed experimental distribution after disposable-VM qualification. No side-loading an unsigned module through an updater.
- No bundled driver, global keyboard hook, mandatory service, or admin requirement for normal operation.
- Package identity/location must be correct for MSIX and per-user MSI. Do not derive user-writable paths from `C:\Program Files\WindowsApps` or claim unvirtualized access solely through `GetEnvironmentVariableW`.
- Start-with-Windows toggle must be implemented appropriately per channel (packaged StartupTask or other supported mechanism, MSI user Startup entry, portable explicitly opt-in). Respect OS Startup Apps settings and user disablement.
- Code signing, hashes, release integrity and Microsoft Store policies are mandatory. **Code signing does not guarantee SmartScreen will never warn**.
- Enforce single-instance semantics within the user session and correct behavior when the same account has portable and installed versions: warn about conflicting active instances and avoid concurrent writes.

### 16.3 Installer and uninstaller scenarios

| Scenario | Required behavior |
|---|---|
| Fresh install | Does not alter taskbar until app runs; clear startup choice. |
| Upgrade running app | Quiesce core safely, snapshot config, update atomically, restart if opted in. |
| Downgrade | Block incompatible schema silently overwriting data; offer read-only/migration-safe fallback. |
| Migration MSI ↔ MSIX | Detect prior data and preserve/export profile; do not assume shared write paths. |
| Portable extraction | Full runtime available without separate Windows App SDK installation. |
| Portable on read-only drive | Binaries may reside read-only; configuration persists in AppData. If AppData itself is unwritable, enter read-only mode and explain the failure. |
| Uninstall | Stop app, restore taskbar where possible, remove startup task/entry; offer choice to keep/delete user data. |
| Interrupted update | Rollback to previous signed package/configuration; no permanent app startup loop. |
| Store certification rejected | Produce owner-visible blocker and compliant package path, not an undocumented bypass. |

### 16.4 Updates

Offline-first: automatic silent cloud access is not required. Provide manual Check for Updates and, if later enabled by user, signed update-notification checking. Update apply requires explicit user consent. Release channels `stable` and `experimental` are clearly separated; experimental Explorer integration is never silently turned on by a stable update.

---

## 17. Failure-mode and edge-case coverage

Each case requires: setup/preconditions, OS/build, trigger, expected fallback, expected state/log entry, observed result, cleanup and evidence artifact.

| ID | Scenario | Required result |
|---|---|---|
| F01 | Explorer normal restart | Rebind/debounce, never loop modifications, preserve user widgets and taskbar fallback. |
| F02 | Explorer crash loop while optional bridge enabled | Disable bridge automatically where possible; safe-mode recovery, no auto-reattach until user consent. |
| F03 | Windows cumulative/feature update | Probe capabilities and invalidate stale allowlists; unsupported effects revert to native. |
| F04 | Taskbar relocated/auto-hide toggled | No clipping or focus theft; preserve animations. |
| F05 | Start/Search/Quick Settings/Widgets flyout | Correct taskbar-state rule, no fighting with OS. |
| F06 | Win+D / show desktop | Widget layer stays in intended desktop stack or safely hides. |
| F07 | Win+Tab / virtual desktop switch | No leaked topmost widget; placements remain logically correct. |
| F08 | Secondary monitor disconnect and reconnect | Bounded reflow; original mapping restored when confidently identified. |
| F09 | Two identical monitors / MST / dock | Resolve duplicate EDIDs using stable path and fallback, not random widget movement. |
| F10 | Mixed DPI and portrait/high refresh | Stable DIP layout, legible rendering, no cursor offset mismatch. |
| F11 | HDR on/off and transparency preference change | Correct colors and conservative fallback; respect accessibility. |
| F12 | GPU driver reset / device removed | Recreate renderer or disable visuals; core/tray remain responsive. |
| F13 | Low-memory or GPU memory pressure | Back off animations/caches, preserve core stability, display diagnostic status. |
| F14 | Background animated wallpaper changes | Widgets remain above wallpaper but below desktop icons if supported. |
| F15 | Wallpaper provider crashes/exits | AetherShell does not restart or hook provider; recovers placement after shell stable. |
| F16 | Desktop icon drag-select/rename/context menu | Widgets do not steal input; icons retain navigation. |
| F17 | Media control under a desktop icon | Ordinary click remains the icon's; media actions require temporary Interactive Mode opened by tray/hotkey. |
| F18 | Spotify or other media player pauses/resumes | Auto-hide/reactivate correctly; no audio samples saved. |
| F19 | Default audio device changes, Bluetooth disconnects | Cancel previous client, rebind or show unavailable, bounded retries. |
| F20 | System audio silent with active render endpoint | Stop costly redraw; avoid indefinitely blocked shutdown. |
| F21 | Protected audio or unsupported channel format | Graceful unavailable visualizer; no noise or fabricated samples. |
| F22 | No GPU/NPU/thermal provider available | Valid missing-state UI, no fake 0%, no crashes. |
| F23 | Laptop dGPU asleep | Sensor provider does not wake it just to populate a widget where avoidable. |
| F24 | Game fullscreen/borderless fullscreen | Conservative shield on that monitor; side monitor conditional. |
| F25 | Anti-cheat product active | No game injection; record unsupported combinations; avoid universal safety claims. |
| F26 | Battery saver / monitor off / lock / sleep | Suspend rendering/capture; robust wake and state refresh. |
| F27 | Remote Desktop/VM session | Detect compositing differences; native/solid fallback where needed. |
| F28 | High Contrast or reduced motion | System colors and reduced animation; settings remain accessible. |
| F29 | Malformed TOML during file watch | Retain last good configuration; show precise validation error. |
| F30 | Config file write interrupted / disk full | Atomic update fails safely, old config preserved, helpful error. |
| F31 | Invalid skin archive / asset overrun | Reject package without executing it; log actionable failure. |
| F32 | Malicious unrestricted trusted Lua worker | Explicit warning and accepted possibility of user-data/other-process damage despite child process; do not claim security sandboxing. |
| F33 | Lua worker loops/hangs/exits/segfaults | Core watchdog detects child exit/heartbeat loss, quarantines, optionally terminates child and remains responsive; intentional same-user attacks remain outside that guarantee. |
| F34 | Settings executable crashes | Core continues, next Settings launch can reconnect. |
| F41 | Lua worker compromised, attempts to send malformed/burst IPC | Core rejects over-sized/invalid messages, rate-limits, preserves tray and taskbar response. |
| F42 | User opens Interactive Mode during game, lock or secure desktop | Suppress temporary control overlay and immediately restore passive input. |
| F43 | Default audio endpoint is a virtual surround device removed mid-playback | Tear down stale WASAPI client; rebind to effective default endpoint or mark unavailable; no busy retry. |
| F44 | Explorer desktop shell lacks expected `WorkerW` hierarchy | Capability check fails without reparenting arbitrary windows; disable widget rendering for affected monitor safely. |
| F45 | Portable and MSI/MSIX instances attempt same AppData config | Session ownership / revision checking prevents simultaneous clobbering; show migration or conflict-resolution guidance. |
| F46 | Taskbar auto-hide edge expands over bottom widget | Respect expanded safe-area policy based on observed geometry and DPI; no universally fixed 48px offset. |
| F47 | Active third-party taskbar customizer modifies Explorer | Stable taskbar backend yields and displays nonblocking warning; leaves other app untouched. |
| F35 | IPC client from another session / malformed data | Reject due to ACL/version/validation; no arbitrary core action. |
| F36 | MSI/MSIX/portable upgrade/downgrade | Preserve and migrate user data, rollback on failure. |
| F37 | Windows Store packaging rejects feature | Compliant stable package; blocker logged, no hidden bypass. |
| F38 | App and other taskbar utility both styling | Yield rather than reapply continuously; user notice. |
| F39 | Multiple Windows interactive sessions | Session-scoped core/tray, data separation, no cross-user media bleed. |
| F40 | App exits / is killed while styling taskbar | Best-effort reset on graceful exit; recovery on subsequent launch; no false guarantee on hard kill. |

---

## 18. Testing strategy and objective acceptance gates

### 18.1 Test layers

1. **Contract/unit:** Rust state transitions; config/profile/schema parsing; layout/binding resolution; theme fallback; IPC envelope; sensor delta arithmetic; audio-format conversion.
2. **Render golden:** Per-theme/skin snapshots at 100%, 125%, 150%, 200%, light/dark/high contrast, grayscale AA, RTL/localized long strings where applicable. Allow tolerance appropriate for Windows text/D2D rendering; do not accept cropped text.
3. **Desktop input:** Icon selection, drag-select, rename, context menu, media-control hit targets, keyboard edit; across mouse DPI/high polling rates without unsupported 0 CPU claim.
4. **Windows matrix:** At least Windows 10 22H2, Windows 11 22H2/23H2/24H2/25H2/26H2; special 26H1 where a qualifying x64 test target exists, and actual OEM/display/driver configurations.
5. **Hardware matrix:** Intel/AMD desktops, iGPU-only laptops, hybrid GPU laptops, mixed-DPI dual/triple monitors, HDR displays, DP docking, Bluetooth/USB audio endpoints, unsupported NPU/no NPU hardware.
6. **Failure injection:** Explorer restart/crash, DComp device removal, audio endpoint disabled, network errors, config corruption, disk full, unavailable sensor, unplugged monitor, IPC timeout, settings crash.
7. **Security:** Dependency audit, installer/signing, path traversal, compressed-package bombs, malicious declarative values, peer IPC authorization, URL redirect/SSRF checks for built-in network widgets; **do not assert unrestricted trusted Lua is sandbox-secure**.
8. **Accessibility:** Screen-reader navigation, keyboard focus, high contrast, reduced motion, Windows text sizing, color contrast in all themes.
9. **Performance:** Workload-separated measurements and regression tracking per Section 14.
10. **Packaging:** Independent installation/upgrade/uninstall/rollback in all five selected distribution paths (MSI, MSIX, Store, WinGet, portable). Store approval is an external release prerequisite.

### 18.2 Required acceptance gates

All stable gates are mandatory for stable v1.0.0 unless expressly marked experimental. POC outcomes must be traceable to a candidate binary hash and exact tested OS build; observed results are not assumed from this PRD.

| Gate | PRD Pass Condition | Current Status | Evidence & Verification Reference |
|---|---|:---:|---|
| **G01 Core startup** | Non-elevated core without mandatory driver/service/Explorer injection, session-scoped singleton, safe shutdown | `PARTIALLY_VERIFIED` | Session-scoped singleton, tray pump, and window lifecycle verified; WM_ENDSESSION test pending |
| **G02 Windows compatibility** | Required Windows 10/11 x64 matrix; honest fallback on unsupported effects | `PARTIALLY_VERIFIED` | Win10/11 build & UBR detection and PerMonitorV2 dynamic DPI scaling verified; multi-OS VM matrix active |
| **G03 Shell isolation** | All stable binaries and packages contain no Explorer extension; no hooks into critical protected processes | `PARTIALLY_VERIFIED` | Non-injection architecture verified in source & binaries; package artifact audit pending |
| **G04 Taskbar recovery** | External effects gated; native fallback; no repeated fighting or Explorer loop | `PARTIALLY_VERIFIED` | Conflict detection yields to customizers; TaskbarCreated recovery verified; Explorer restart VM pixel test pending |
| **G05 Experimental qualification** | Any opt-in Explorer extension: disposable-VM tests, then dedicated recoverable hardware qualification before limited experimental distribution | `NOT_APPLICABLE` | No experimental Explorer extension in stable v1 core |
| **G06 Desktop layering** | Wallpaper below passive widgets below desktop icons on qualifying builds; otherwise visibly unsupported/disabled | `PARTIALLY_VERIFIED` | WorkerW/Progman layering & icon passthrough verified; Win+D non-invasive shell hook stacking active |
| **G07 Temporary Interactive Mode** | Tray/hotkey opens bounded overlay, user action works; Escape/timeout/focus change restores passive icon input | `PARTIALLY_VERIFIED` | Global hotkey, pointer hit-testing, action dispatch & safe dismissal verified; VM video record pending |
| **G08 Sensor honesty** | Missing counters collapse/show unknown; optional external providers; tested non-waking sampling | `PARTIALLY_VERIFIED` | Native CPU/RAM/disk/battery/net telemetry implemented; honest missing states verified |
| **G09 Local adaptive audio** | Default-on local WASAPI, endpoint recovery, battery 15 FPS default, AC 30 FPS, configurable 60 FPS, no persistent samples | `PARTIALLY_VERIFIED` | In-memory WASAPI shared-loopback spectrum, power-aware polling & IMMNotificationClient migration verified |
| **G10 Lua worker reliability and trust** | Script host separate and lazy; explicit digest-bound consent; crash/hang detected without core-blocking; IPC invalid payload rejected | `PARTIALLY_VERIFIED` | Out-of-process Lua in Job Object, SHA-256 trust & continuous revalidation verified across 22 tests |
| **G11 WinUI Settings independence** | C++/WinRT process on-demand, exits on close, survives lost IPC, core survives crash | `PARTIALLY_VERIFIED` | C++/WinRT WinUI 3 standalone Settings, 2-way IPC & exit on close verified; fault injection VM test pending |
| **G12 Schema/backup** | All TOML samples parse; migrations reversible; installed and portable share intended AppData store with conflict protection | `PARTIALLY_VERIFIED` | TOML config parsing, atomic ReplaceFileW & rollback verified; schema v1->v2 migrations pending |
| **G13 Performance** | Real measured workloads meet ratified budgets on baseline matrix; no false exact-zero claims | `PARTIALLY_VERIFIED` | Genuine multi-process runtime benchmark harness & Ryzen 5 baseline recorded; optimization active |
| **G14 Accessibility** | Keyboard, Narrator/UIA, high contrast, scaling, reduced motion, localized truncation rules | `PARTIALLY_VERIFIED` | High-contrast palettes & WinUI UIA controls verified; desktop widget UIA provider pending |
| **G15 Five-channel release** | **MSI, MSIX, Microsoft Store approved listing, WinGet, self-contained portable** fully qualified; no partial stable release | `DEFERRED` | Packaging scripts exist; public Store/WinGet/signing deferred per project release roadmap |
| **G16 License/privacy** | **Apache-2.0** LICENSE and dependency notices; no unsolicited analytics or raw audio upload | `PARTIALLY_VERIFIED` | Apache-2.0 license & volatile audio processing verified; path remapping verified in CI |
| **G17 Safe Mode and recovery** | Restart safe mode, widget/worker quarantine, Explorer cleanup, rollback, IPC errors, update recovery | `PARTIALLY_VERIFIED` | Safe Mode, worker quarantine, Explorer recovery & D2D device loss recovery verified |
| **G18 Strict no-go and evidence** | No unverified critical behavior represented as supported; no physical-host disruptive tests; all gates signed off | `PARTIALLY_VERIFIED` | Truthful gate accounting enforced; disruptive shell tests restricted to disposable VMs |
| **G19 Coexistence** | Detected competing taskbar style controller yields; does not terminate or modify third-party process | `PARTIALLY_VERIFIED` | Conflict detection deterministically yields to competing customizers without process mutation |
| **G20 Packaging identity** | Same user data semantics across MSI/MSIX/Store/portable, startup toggles, signed migrators and rollback | `PARTIALLY_VERIFIED` | Canonical %LOCALAPPDATA% storage layout verified clean at 0 items; multi-channel VM migration pending |

### 18.3 Research / experiment matrix and environment boundaries

**Test environment boundaries:** Any action that changes Explorer/taskbar state, window parenting/z-order, app/hook injection, registry/service state, installs packages, forces GPU removal, deliberately hangs a process, or directly tests game anti-cheat is **disruptive** and **VM-only**. Collecting non-invasive performance traces on a physical machine is permissible only where explicitly authorized and after controlling confounding workloads. Dedicated recoverable-hardware qualification for an experimental Explorer *distribution* does not authorize direct disruptive testing on the daily-use machine.

| POC ID | Question / method | Required environment | Acceptance and fallback |
|---|---|---|---|
| POC-TB-01 | Can external API calls visibly change native taskbar effects? Control A/B pixels, child XAML, transient visual artifacts, exact build | Disposable Win10/11 x64 VMs; no other customizer for baseline | Per-effect, per-build observed pass; otherwise `native_noop`, no universal impossibility claim |
| POC-TB-02 | Is native floating geometry stable? Taskbar UI, auto-hide, keyboard, Explorer recovery | Disposable VM only | Nonblocking experimental feature, no stable v1 promise |
| POC-TB-03 | Is optional XAML bridge recoverable without crash loops? | VM-only initial; later dedicated recoverable hardware qualification without expanding VM-only authorization | Signed separate experimental channel or do not distribute |
| POC-DS-01 | Which `Progman`/`WorkerW` desktop-host variants work with icons/live wallpaper? | Disposable VM | Exact stacking pass with Win+D/Explorer changes; else disable widgets on that configuration |
| POC-DS-02 | Does temporary Interactive Mode preserve icon hits/focus? | Disposable VM | Bounded overlay only after user request, Escape/time/focus returns control |
| POC-RD-01 | What do per-widget DComp/Direct2D surfaces consume on iGPU and other GPUs? | VM for device-loss manipulations; consented non-invasive physical measurement for power/graphics | Measure real private/GPU memory, latency, rendering failure |
| POC-UI-01 | What does self-contained WinUI 3/C++ Settings cost on Win10/11? | Clean Windows VMs; non-invasive physical benchmark if authorized | Measured size and memory, clean exit; framework remains WinUI 3 |
| POC-LUA-01 | Does a separate Lua worker survive infinite loop/native fault without core freeze? | Disposable VM | Core tray remains responsive; worker shutdown/quarantine; no security-isolation promise |
| POC-PK-01 | Can MSI/MSIX/Store/portable share canonical AppData store and migrate safely? | VM only (installation and writes) | Transactional integrity, data ownership, full channel eligibility |
| POC-GM-01 | Does Gaming Shield avoid overlays and excessive activity? | Disposable VM for program/window mutations; further approved non-invasive measurements | No untested anti-cheat certification |
| POC-AU-01 | How does WASAPI react to virtual endpoint removal, silence, DC/AC changes? | VM for fault injection; authorized non-invasive physical battery/audio measurement | Cancellation, no leaking buffers, adaptive profile as designed |
| POC-SN-01 | Which GPU/NPU/thermal metrics work without drivers/admin? | Read-only device inventory plus controlled VM/hardware validation | Unsupported states explicit, optional external bridge |
| POC-TB-04 | Does auto-hide move into bottom widget safe area? | VM for active UI/layout tests | Measured expanded extent per display rather than static 48 px |
| POC-WIN-01 | OS build, Explorer version, GPU driver and WDDM variation | VM matrix and repeatable read-only inventory | Revalidate feature allowlist on update |

**POC results must not be prefilled PASS.** Agent observations are baseline inputs, not completed product integration tests.

### 18.4 Evidence format

Each POC and gate gets a record with `id`, `candidate_commit`, `Windows version/build`, `hardware`, `driver`, `package_channel`, `steps`, `expected`, `observed`, `artifact_paths`, `repeat_count`, `known_limitations`, `pass_fail`, `reviewer`, `date`, and `followup_issue`. Avoid storing sensitive desktop screenshots in public CI by default; redact or use synthetic desktop fixtures.

---

## 19. Roadmap, milestones and dependencies

This roadmap is **dependency-driven** and reflects verified architectural milestones across the implementation lifecycle.

### Phase 0 — Requirements and Windows feasibility (Status: ✅ Completed)

- Established Git repository, documentation hierarchy, toolchain and Windows/SDK support matrix.
- Built standalone taskbar external appearance prototype with robust `native_noop` fallback; inspected exact Windows build behavior.
- Built desktop host wallpaper/icons stacking prototype plus temporary Interactive Mode; verified Win+D and icon input handling.
- Built minimal WinUI 3 C++/WinRT settings launcher in separate process; validated self-contained execution.
- Benchmarked Rust/DirectComposition widget host under realistic conditions.

### Phase 1 — Repository and baseline runtime (Status: ✅ Completed)

- Implemented Cargo workspace (13 member crates), C++ Settings project, named pipe IPC contract, structured logging, TOML parser, safe mode, and schema validation.
- Built session-scoped tray, lifecycle message pump, single-instance activation, and non-invasive taskbar discovery.
- Implemented system capability probes; zero Explorer injection in standard binaries.
- Staged initial clean-VM MSI/MSIX packaging scripts plus portable layout specification.

### Phase 2 — Taskbar and desktop host (Status: ✅ Completed)

- Implemented validated `native_noop` and `external_appearance` backends with conflict detection.
- Implemented per-monitor taskbar state machine with fullscreen, flyout, auto-hide, and accessibility overrides.
- Built desktop-only passive visuals between wallpaper and icons with safe fallback; implemented time-bounded Interactive Mode (`Win+Ctrl+A`) with spatial hit-testing.
- Implemented Explorer restart recovery (`TaskbarCreated`) and third-party customizer yielding.

### Phase 3 — Widget and visual design platform (Status: 🔶 Partially Completed)

- Implemented Direct2D/DirectWrite declarative scene renderer, live binding dictionary, and theme engine with 6 semantic palettes.
- Built-in clock, system telemetry, media session, audio spectrum, and resource primitives functional.
- Schema validation, layout recovery, and high-contrast accessibility support active.
- *Pending:* Authorable third-party skin package distribution format.

### Phase 4 — Telemetry, media, audio and trusted Lua (Status: ✅ Completed)

- Implemented Windows native telemetry samplers (CPU, RAM, storage, network, battery) with honest missing-metric reporting.
- Implemented WASAPI loopback capture thread, Cooley-Tukey 512-point FFT, logarithmic band grouping, and endpoint migration (`IMMNotificationClient`).
- Implemented adaptive presentation loop with dirty-state presentation gating and silence normalization.
- Implemented out-of-process Lua 5.4 child worker (`aethershell-script-host.exe`) in Job Object memory quotas, pre-execution SHA-256 digest trust verification, continuous runtime watchdog revalidation, and crash quarantine circuit breaker.

### Phase 5 — Windows and packaging qualification (Status: 🔶 In Progress)

- Automated multi-process runtime benchmark harness implemented; bare-metal AMD Ryzen 5 5500U baseline established.
- Disposable VM and Windows Sandbox provisioning tooling implemented (`scripts/provision_disposable_vm.ps1`) allocating 2048 MB RAM / 2 vCPUs under strict preflight safety.
- Reconciled automated workspace test inventory to 300 unique Cargo tests + 36 PowerShell regression tests = 336 passing automated tests.
- *In Progress:* Multi-OS VM matrix qualification (Win10 22H2, Win11 22H2/23H2/24H2/26H2) and packaging channel qualification (MSI, MSIX, Store, WinGet, portable).

### Phase 6 — Optional advanced taskbar laboratory (Status: ⬜ Not Started)

- Research floating geometry and Explorer/XAML bridge in disposable test virtual machines.
- Nonblocking research track; stable v1 does not depend on floating geometry or in-process Explorer modules.

### Release and no-go governance

An individual unsupported experimental feature can be excluded while core v1 proceeds. However, **the required launch distribution channels, Windows 10/11 compatibility, default audio visualization, desktop layer, native x64 architecture, user-facing safeguards, and packaging commitments remain strict release requirements**. If any required channel or gate cannot pass empirical qualification, release candidates maintain preview status until verified. Partial-channel distribution is not considered stable v1.0.0; Microsoft Store approval is a mandatory release condition.

---

## 20. Implementation backlog and release artifact checklist

### 20.1 Implementation milestones sequence

- [x] **Milestone 1:** Establish Windows toolchain, Rust core runtime, C++ Settings IPC contract, and clean process shutdown.
- [x] **Milestone 2:** Produce exact Windows build/edition capability detection, registry UBR reading, and diagnostics output.
- [x] **Milestone 3:** Prototype external taskbar effects and implement verified `native_noop` and `external_appearance` backends.
- [x] **Milestone 4:** Implement desktop host window (`WorkerW`/`Progman` discovery), wallpaper/icon layering, and temporary Interactive Mode (`Win+Ctrl+A`).
- [x] **Milestone 5:** Build and verify standalone WinUI 3 C++/WinRT Settings executable with 2-way IPC synchronization and clean exit on close.
- [x] **Milestone 6:** Implement unit-tested declarative configuration parser, TOML serialization, atomic ReplaceFileW persistence, and backup rotation.
- [x] **Milestone 7:** Implement Direct2D/DirectWrite rendering pipeline with dirty-state presentation gating and measure real idle costs.
- [x] **Milestone 8:** Implement WASAPI local spectrum loopback with 512-point FFT, endpoint migration (`IMMNotificationClient`), and default-on UX.
- [x] **Milestone 9:** Implement native telemetry samplers (CPU, RAM, storage, network, battery) with honest missing-state reporting.
- [x] **Milestone 10:** Implement per-monitor Gaming Shield fullscreen detection and conflict-yielding behavior.
- [x] **Milestone 11:** Implement out-of-process Lua 5.4 child worker (`aethershell-script-host.exe`), SHA-256 digest trust verification, continuous runtime watchdog revalidation, and crash quarantine circuit breaker.
- [ ] **Milestone 12:** Complete multi-OS disposable VM qualification across target Windows build families and finalize 5-channel packaging (MSI, MSIX, Store, WinGet, portable) under Apache-2.0.

### 20.2 Required artifacts for release completion

- [x] Stable source workspace, compilable Rust core and C++/WinRT Settings, repository governance, reproducible build instructions.
- [x] Public JSON schemas, complete built-in TOML examples, and migration fixtures.
- [x] Six theme palettes with semantic color tokens and high-contrast accessibility support.
- [x] Capability probe output, Windows build detection, conflict detection, and non-injection architecture verification.
- [x] Genuine multi-process runtime benchmark harness and bare-metal AMD Ryzen 5 5500U baseline ledger.
- [x] Automated test inventory tool (`scripts/test_inventory.ps1`) reconciling 300 unique Cargo tests + 36 PowerShell regression tests = 336 passing automated tests.
- [ ] Multi-OS disposable VM qualification records across Windows 10 22H2 through Windows 11 26H2.
- [ ] Signed MSI/MSIX packages, self-contained portable bundle, Microsoft Store certified listing, and WinGet manifest publication.
- [x] Apache-2.0 `LICENSE` text, `NOTICE.md`, `SECURITY.md`, and clean dependency audit.
- [ ] End-user documentation covering installation, configuration, safe mode, and uninstallation across all 5 distribution channels.
- [ ] Final 20-gate release sign-off ledger with signed artifact hashes.

---

## Technical Appendices

For Microsoft platform API source references [S01]–[S17], baseline physical hardware observations, and technical proof-of-concept (POC) tracking, see:
👉 [docs/prd/PRD_APPENDICES.md](docs/prd/PRD_APPENDICES.md)

**End of AetherShell Product Requirements Document — Product v1.0.0; Specification Baseline 2026-10-29 / v1.0.**