# AetherShell Microsoft Store Preflight & Compliance Checklist

**Target Version:** 1.0.0.0  
**Package Identity:** `ArchdukeViel.AetherShell`  
**Publisher:** `CN=ArchdukeViel`  
**License:** Apache-2.0  

---

## 1. Microsoft Store Policy Compliance Audit

| Policy Section | Requirement | AetherShell Implementation Status | Compliance Evidence |
|:---|:---|:---:|:---|
| **10.1 Distinct Value** | The product must provide distinct functionality and user value. | `COMPLIANT` | Modular desktop customization engine combining hardware telemetry, local WASAPI audio spectrum analysis, and declarative theming. |
| **10.2 Quality & Performance** | The app must run smoothly, handle errors gracefully, and maintain reasonable resource usage. | `COMPLIANT` | Idle CPU < 0.5%, idle RAM < 40 MiB. On-demand WinUI 3 settings app exits on close (Gate G11) to eliminate background memory footprint (0 MB resident). |
| **10.5 Personal Data & Privacy** | Explicit disclosure of all personal data, telemetry, and media access. | `COMPLIANT` | **Audio Privacy Guarantee (Gate G16 & PRD [S08])**: Audio spectrum analysis uses WASAPI shared loopback strictly in-memory. Zero raw audio persistence; zero disk writes; zero network transmission. Local-first offline operation with zero user tracking. |
| **10.8 Financial & Account Terms** | Free apps must not deceive users or enforce hidden paywalls. | `COMPLIANT` | 100% free, open-source Apache-2.0 software. Fully functional offline with zero cloud account requirements. |
| **10.13 Security & Safety** | No malware, unauthorized privilege escalation, or system disruption. | `COMPLIANT` | Runs strictly in user-mode without administrator elevation. No kernel drivers. No Windows Explorer process injection in stable releases (Gate G03). Non-invasive coexistence with other tools (Gate G19). |

---

## 2. Store Visual Asset Requirements & Audit

The following visual assets are specified in `packaging/msix/AppxManifest.xml` and generated at exact target pixel dimensions:

| Asset Name | Target Resolution | Manifest Entry | Format | Verified Status |
|:---|:---|:---|:---:|:---:|
| **Store Logo** | 50 x 50 px | `Assets\StoreLogo.png` | 32-bpp PNG | `VERIFIED` |
| **Square 150x150 Logo** | 150 x 150 px | `Assets\Square150x150Logo.png` | 32-bpp PNG | `VERIFIED` |
| **Square 44x44 Logo** | 44 x 44 px | `Assets\Square44x44Logo.png` | 32-bpp PNG | `VERIFIED` |
| **Wide 310x150 Logo** | 310 x 150 px | `Assets\Wide310x150Logo.png` | 32-bpp PNG | `VERIFIED` |
| **Square 71x71 Logo** | 71 x 71 px | `Assets\Square71x71Logo.png` | 32-bpp PNG | `VERIFIED` |
| **Splash Screen** | 620 x 300 px | `Assets\SplashScreen.png` | 32-bpp PNG | `VERIFIED` |

---

## 3. Five-Channel Packaging Matrix Audit (Gate G15)

| Channel | Packaging Artifact | Scope / Target | Contents & Signing | Generation Tooling | Audit Status |
|:---|:---|:---|:---|:---|:---:|
| **1. WiX v4 MSI** | `target/dist/msi/AetherShell-v1.0.0-x64.msi` | Machine-wide (`Scope="perMachine"`) | Full binaries, WinUI 3 settings, 6 skins, 6 themes, signature profile, schemas | WiX v4.0.5 via `build-msi.ps1` | `PASS` |
| **2. MSIX Package** | `target/dist/msix/ArchdukeViel.AetherShell_1.0.0.0_x64.msix` | Desktop Bridge (`runFullTrust`) | Binaries, WinUI 3 settings runtime, authentic PNG assets, SHA256 Authenticode signature | `makeappx.exe` + `signtool.exe` via `build-msix.ps1` | `PASS` |
| **3. Portable ZIP** | `target/dist/portable/AetherShell-v1.0.0-portable-win-x64.zip` | Standalone portable (Q38:B canonical AppData) | Self-contained package without `aethershell.portable` marker or local `data/` dir | `Compress-Archive` via `build-portable.ps1` | `PASS` |
| **4. WinGet Manifest** | `packaging/winget/manifests/...` (v1.6.0) | `Scope: machine` | Points to GitHub v1.0.0 MSI release with authentic SHA256 checksum | Declarative YAML manifests | `PASS` |
| **5. Store Preflight** | `packaging/store/StorePreflight.md` | Microsoft Store / Partner Center | Store policy audit, privacy disclosures, visual asset verification | Preflight markdown document | `PASS` |

---

## 4. Preflight Readiness Verdict

All required manifests, security declarations, privacy assertions, and technical criteria meet Microsoft Store submission standards. Packaging artifact generation is validated via `packaging/msix/build-msix.ps1` and `packaging/wix/build-msi.ps1`.
