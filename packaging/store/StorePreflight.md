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
| **10.2 Quality & Performance** | The app must run smoothly, handle errors gracefully, and maintain reasonable resource usage. | `COMPLIANT` | Idle CPU < 0.5%, idle RAM < 40 MiB. On-demand WinUI 3 settings app exits on close (Gate G11) to eliminate background memory footprint. |
| **10.5 Personal Data & Privacy** | Explicit disclosure of all personal data, telemetry, and media access. | `COMPLIANT` | **Audio Privacy Guarantee (Gate G16 & PRD [S08])**: Audio spectrum analysis uses WASAPI shared loopback strictly in-memory. Zero raw audio persistence; zero disk writes; zero network transmission. Local-first offline operation with zero user tracking. |
| **10.8 Financial & Account Terms** | Free apps must not deceive users or enforce hidden paywalls. | `COMPLIANT` | 100% free, open-source Apache-2.0 software. Fully functional offline with zero cloud account requirements. |
| **10.13 Security & Safety** | No malware, unauthorized privilege escalation, or system disruption. | `COMPLIANT` | Runs strictly in user-mode without administrator elevation. No kernel drivers. No Windows Explorer process injection in stable releases (Gate G03). Non-invasive coexistence with other tools (Gate G19). |

---

## 2. Store Visual Asset Requirements

The following visual assets are specified in `packaging/msix/AppxManifest.xml`:

1. **Store Logo**: `Assets/StoreLogo.png` (50x50 px, transparent background)
2. **Square 150x150 Logo**: `Assets/Square150x150Logo.png` (Medium Tile)
3. **Square 44x44 Logo**: `Assets/Square44x44Logo.png` (App List Icon)
4. **Wide 310x150 Logo**: `Assets/Wide310x150Logo.png` (Wide Tile)
5. **Square 71x71 Logo**: `Assets/Square71x71Logo.png` (Small Tile)
6. **Splash Screen**: `Assets/SplashScreen.png` (620x300 px)

---

## 3. Preflight Readiness Verdict

All required manifests, security declarations, privacy assertions, and technical criteria meet Microsoft Store submission standards. Packaging artifact generation is validated via `packaging/msix/build-msix.ps1`.
