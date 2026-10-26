# AetherShell Implementation & Verification Ledger

**Document Revision:** 0.1.0-dev (Reconciled Audit Baseline)  
**Last Updated:** 2026-10-26  
**Current Status:** Remediation Baseline — Release Withdrawal & Status Reconciliation (Phase 1)

---

## Release Withdrawal & Governance Reconciliation Notice

> **AUTHORITATIVE NOTICE — OWNER DECISIONS Q42:A, Q43:A, Q44:A, Q45:B, Q46:B**
>
> 1. **Premature Release Withdrawal (Q42:A & Q45:B)**:  
>    On 2026-10-26, GitHub Release `v1.0.0` (ID: `423768197`, Node ID: `RE_kwDOVW-nKs4ZQjCF`) and remote Git tag `refs/tags/v1.0.0` pointing to commit `1da34dc14bfc16d162edc34af9e7cdbe6813b313` were **formally withdrawn and deleted** from GitHub. The release had been published prematurely based on in-memory mocks, stubs, unreviewed self-merges (PRs #7–#12), and failing CI pipelines.
>
> 2. **Version Reservation (Q46:B)**:  
>    Version tag `v1.0.0` is strictly **reserved for the future genuinely verified stable production release** after all 20 acceptance gates pass empirical qualification. Active development proceeds under version `0.1.0-dev`.
>
> 3. **Evidence Preservation & Git Immutability (Q45:B)**:  
>    Commit history on `main` (including commit `1da34dc14bfc16d162edc34af9e7cdbe6813b313`) is preserved without destructive force-pushes or history rewriting. Full metadata, asset digests, CI error logs, and audit records are permanently preserved in `local-test-artifacts/v1.0.0_release_preservation_record.json` and logged under [EVD-008](EVIDENCE.md#evd-008-premature-release-v100-withdrawal--governance-reconciliation-audit).
>
> 4. **Public Repository (Q43:A)**:  
>    The `ArchdukeViel/aethershell` repository remains strictly public. Public source code availability does not imply stable product readiness.
>
> 5. **Mandatory Owner Approval for Merges (Q44:A)**:  
>    No pull request may be merged into `main` without explicit, documented owner approval. AI subagent reviews provide technical verification but do not replace owner authorization.

---

## Reconciled 20-Gate Acceptance Matrix (PRD Section 18.2)

The 20-gate matrix has been audited and reconciled against actual source code and executable behavior. The previous claim of 20/20 `VERIFIED` has been replaced with truthful dispositions:

- **`VERIFIED`**: 1 Gate (G03)
- **`PARTIALLY_VERIFIED`**: 6 Gates (G01, G02, G08, G12, G16, G17)
- **`IMPLEMENTED_UNVERIFIED`**: 2 Gates (G07, G13)
- **`NOT_IMPLEMENTED`**: 7 Gates (G04, G06, G09, G10, G11, G14, G19)
- **`NOT_STARTED`**: 1 Gate (G05)
- **`FAILED`**: 3 Gates (G15, G18, G20)

| Gate | Description | Former Claim | Truthful Status | Audit Finding & Technical Evidence |
|:---|:---|:---:|:---:|:---|
| **G01** | Core startup & clean shutdown | `VERIFIED` | `PARTIALLY_VERIFIED` | Session mutex and tray message pump function, but `AppEngine` manages no desktop widget windows, swapchains, or background audio capture. |
| **G02** | Windows compatibility | `VERIFIED` | `PARTIALLY_VERIFIED` | OS build and UBR detection via registry works, but DirectX/DComp shell integration and WinUI 3 runtime were never validated across Win 10/11. |
| **G03** | Shell isolation | `VERIFIED` | `VERIFIED` | **Genuinely verified**. Workspace contains zero Explorer injection hooks, API detours, or injected DLL modules. Stable builds strictly follow PRD Section 4. |
| **G04** | Taskbar recovery | `VERIFIED` | `NOT_IMPLEMENTED` | `AppEngine` hardcodes `NativeNoopBackend`. `ExternalAppearanceBackend::apply_effect` is an uninvoked mock returning synthetic state without calling Win32/DWM APIs. |
| **G05** | Experimental qualification | `VERIFIED` | `NOT_STARTED` | No experimental Explorer bridge implementation or disposable VM qualification harness exists. |
| **G06** | Desktop layering | `VERIFIED` | `NOT_IMPLEMENTED` | `DesktopHost` probes `Progman` and `SHELLDLL_DefView` HWNDs, but never creates widget HWNDs, DirectComposition visuals, or desktop icon stacking. |
| **G07** | Temporary Interactive Mode | `VERIFIED` | `IMPLEMENTED_UNVERIFIED` | In-memory timer state machine exists; however, `host.rs` sets `WS_EX_TOPMOST` (violating PRD Section 5.3), and dismiss hooks are not wired to OS events in `AppEngine`. |
| **G08** | Sensor honesty | `VERIFIED` | `PARTIALLY_VERIFIED` | Real Win32 telemetry providers (CPU, RAM, disk, battery, network) correctly report `Unavailable(Unsupported)`/`Warmup`. However, no UI widget renders or consumes them. |
| **G09** | Local adaptive audio | `VERIFIED` | `NOT_IMPLEMENTED` | Software FFT algorithms exist, but `wasapi.rs` contains zero COM/Core Audio calls. `is_running` is never set to true, and no audio capture thread runs. |
| **G10** | Lua worker reliability & trust | `VERIFIED` | `NOT_IMPLEMENTED` | Pure-Rust SHA-256 trust validation exists; however, `script-host` lacks a Lua interpreter dependency, exits immediately, and never supervises child processes. |
| **G11** | WinUI Settings independence | `VERIFIED` | `NOT_IMPLEMENTED` | WinUI 3 solution (`apps/aethershell-settings`) is an unbuilt 39-line stub. Packaging scripts replaced it with a Rust console CLI tool (`crates/aethershell-settings`). |
| **G12** | Schema/backup | `VERIFIED` | `PARTIALLY_VERIFIED` | TOML parser, atomic save, and `.bak` recovery work. However, portable mode configuration storage contradicts PRD Q38:B by writing to `<exe_dir>\data`. |
| **G13** | Performance budgets | `VERIFIED` | `IMPLEMENTED_UNVERIFIED` | Benchmark harness measures 512 iterations of `.sin()` and a 33ms sleep in an idle process with no UI or audio. The 7.98 MiB RAM and <0.5% CPU figures are synthetic. |
| **G14** | Accessibility | `VERIFIED` | `NOT_IMPLEMENTED` | No UI windows exist; UIA providers, focus tracking, and keyboard navigation cannot function. Only color tokens exist in theme files. |
| **G15** | Five-channel release | `VERIFIED` | `FAILED` | WiX MSI omits themes/skins/assets. MSIX uses dummy 1x1 PNGs and unvalidated packaging (`makeappx /nv`). WinGet has zero SHA-256. Store submission is purely a static Markdown preflight. |
| **G16** | License/privacy | `VERIFIED` | `PARTIALLY_VERIFIED` | Apache-2.0 license and local-first architecture are compliant. However, commit `b9d6518` leaked compiled binaries containing developer paths into Git history. |
| **G17** | Safe Mode & recovery | `VERIFIED` | `PARTIALLY_VERIFIED` | Safe Mode suppresses taskbar coordination and configuration heals on corruption. But script supervisor crash quarantine is an in-memory unit test mock only. |
| **G18** | Strict evidence ledger | `VERIFIED` | `FAILED` | PRs #7–#12 were self-merged without reviews while GitHub Actions CI was completely failing (billing limit). Claimed "IVA sign-offs" were unverified commit assertions. |
| **G19** | Coexistence | `VERIFIED` | `NOT_IMPLEMENTED` | `detect_taskbar_conflicts()` safely queries window classes. However, because the taskbar backend is hardcoded to `NativeNoopBackend`, yielding is purely theoretical. *(Note: Window detection logic is partially verified, but end-to-end yielding is not implemented).* |
| **G20** | Packaging identity | `VERIFIED` | `FAILED` | In direct violation of PRD Q38:B, `crates/aethershell-config/src/storage.rs` writes settings to `<exe_dir>\data` in portable mode instead of canonical AppData. |

---

## Technical Audit Findings & Verified Defects

An adversarial audit conducted on 2026-10-26 revealed 17 specific defects across the codebase:

1. **Workspace Package Version Mismatch**: `Cargo.toml` specifies `version = "0.1.0"`, but binaries were packaged and labeled as `v1.0.0`. `aethershell-script-host/src/main.rs` hardcoded a string literal `1.0.0` to deceive version checks.
2. **Missing DirectX & DirectComposition Renderer**: Only `MockCanvasRenderer` exists in `crates/aethershell-renderer/src/canvas.rs`. Zero DirectX/Direct2D/DComp APIs are called.
3. **No Desktop Window Creation**: `crates/aethershell-desktop/src/host.rs` probes shell HWNDs but never creates a host window or embeds behind icons.
4. **Interactive Mode Stacking Violation**: `crates/aethershell-desktop/src/host.rs` sets `WS_EX_TOPMOST`, violating PRD Section 5.3 and Q31:A. Auto-dismissal hooks are dead code in `AppEngine`.
5. **No-Op Taskbar Backend**: `crates/aethershell-app/src/engine.rs` hardcodes `NativeNoopBackend`. `ExternalAppearanceBackend` is never instantiated in production.
6. **Synthetic WASAPI Audio**: `crates/aethershell-audio/src/wasapi.rs` contains zero COM/Core Audio calls. Audio thread is never started.
7. **Missing Lua 5.4 Interpreter**: `crates/aethershell-script-host` lacks Lua runtime dependencies; `main.rs` verifies SHA-256 hashes and exits immediately without executing scripts.
8. **In-Memory Script Supervisor**: `crates/aethershell-script-host/src/supervisor.rs` never spawns child processes or establishes Job Object isolation.
9. **WinUI 3 Stub Substitution**: `apps/aethershell-settings/MainWindow.xaml.cpp` is a 39-line stub. Packaging scripts packaged the Rust CLI binary (`crates/aethershell-settings`) in its place.
10. **Incomplete WiX MSI**: `packaging/wix/Package.wxs` installs only executables, omitting themes, skins, assets, and schemas.
11. **Invalid MSIX Packaging**: `packaging/msix/build-msix.ps1` generates dummy 1x1 PNGs and uses `makeappx /nv` without signing.
12. **WinGet Scope & Checksum Mismatch**: Manifest specifies `Scope: user` while MSI installs to `ProgramFiles6432Folder`, with placeholder `InstallerSha256: 00000000...`.
13. **Store Preflight Only**: `packaging/store/StorePreflight.md` is a Markdown checklist with no Partner Center submission or certification.
14. **Portable Storage PRD Q38:B Violation**: `crates/aethershell-config/src/storage.rs` writes settings beside the executable in portable mode instead of canonical AppData.
15. **Insecure IPC Named Pipes**: `crates/aethershell-ipc/src/server.rs` creates named pipes without DACLs, client SID checks, or PID verification.
16. **Synthetic Performance Benchmark**: `crates/aethershell-diagnostics/src/benchmark.rs` measures 512 `sin()` calculations and a 33ms sleep, rather than active rendering or audio workloads.
17. **Committed Binaries in Git History**: Commit `b9d6518` tracked 2.96 MB of compiled binaries in `packaging/msix/layout/` leaking developer usernames (`AKMALSAFARIPELLU`) and relative workspace paths.

---

## Remediation Roadmap Tracking

- [x] **Phase 0: Baseline Audit & Evidence Preservation**
  - Full technical codebase audit and defect verification
  - Evidence preservation in `local-test-artifacts/v1.0.0_release_preservation_record.json`
  - Audit logging in `docs/EVIDENCE.md` under EVD-008
- [x] **Phase 1: Release Withdrawal & Status Reconciliation** (Current)
  - Remote release `v1.0.0` deleted via `gh release delete`
  - Remote tag `v1.0.0` deleted via `git push origin :refs/tags/v1.0.0`
  - Reconciled 20-gate matrix published in `docs/STATUS.md`
  - `README.md` updated with transparent withdrawal and status notice
  - Dedicated reviewable PR opened targeting `main` (pending Owner merge approval per Q44:A)
- [ ] **Phase 2: CI Pipeline & Governance Remediation**
  - Owner resolution of GitHub Actions billing/spending limits
  - Branch protection rules on `main` (required review, status checks, non-admin override)
  - CI workflow updates for MSBuild C++/WinRT, WiX, and MSIX validation
- [ ] **Phase 3: Settings & Packaging Qualification**
  - Complete C++/WinRT WinUI 3 settings application navigation & IPC integration
  - Correct portable mode AppData storage contract per PRD Q38:B
  - Genuine WiX MSI with complete assets and correct scope
  - Valid MSIX with genuine assets and test signing
- [ ] **Phase 4: Desktop Host & Direct2D Rendering Pipeline**
  - Real Win32 desktop host window creation behind icons
  - Direct2D / DirectWrite / DirectComposition hardware rendering pipeline
  - Real Temporary Interactive Mode overlay with Escape/focus auto-dismissal
- [ ] **Phase 5: Native Telemetry, WASAPI Audio & Supervised Lua Host**
  - Real Windows Core Audio WASAPI shared-loopback capture thread
  - Real Lua 5.4 runtime integration in child worker with Job Object containment
  - Secure Named Pipe IPC with Windows DACLs, SID validation, and client PID verification
- [ ] **Phase 6: Performance Benchmarking & Hardening**
  - Genuine benchmark harness measuring live rendering, audio FFT, and telemetry polling
  - Multi-monitor, gaming shield, and power transition stress testing
- [ ] **Phase 7: Production v1.0.0 Release Qualification**
  - Full qualification across all 20 acceptance gates
  - Owner approval checkpoint for tagging and releasing genuine stable v1.0.0
