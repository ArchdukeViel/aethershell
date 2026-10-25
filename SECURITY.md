# Security Policy

## Supported Versions

| Version | Supported | Notes |
|:---|:---:|:---|
| 1.0.0-dev | Yes | Active development |

## Reporting a Vulnerability

If you discover a security vulnerability or security-sensitive defect in AetherShell, please report it responsibly:

- Do **not** open a public GitHub issue for confidential vulnerability disclosures.
- Submit reports to project maintainers via GitHub Private Vulnerability Reporting or via secure email.
- Include detailed reproduction steps, target OS build, and environment configuration.

## Core Security & Isolation Model

- **Standard User Privileges**: AetherShell runs in user mode without requiring administrative elevation or kernel drivers.
- **Out-of-Process Script Execution**: Lua scripts run in a separate supervised child process (`aethershell-script-host.exe`). While process separation provides crash isolation, unrestricted scripts execute under standard user permissions; third-party scripts require explicit user consent and digest verification.
- **Session-Bound Local IPC**: Named pipe IPC endpoints enforce current user session security descriptors and protocol-level message size limits.
- **Zero Audio Storage**: Loopback audio capture samples are processed in volatile memory only and discarded immediately after FFT processing.
