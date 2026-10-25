# Contributing to AetherShell

Thank you for your interest in contributing to AetherShell!

## Development Principles

1. **Truth in Evidence**: Never claim unverified functionality or fabricate test outputs. Features must be proven by reproducible automated tests or hardware validation records.
2. **Safety First**:
   - Never inject into critical Windows processes (`explorer.exe`, `dwm.exe`, `csrss.exe`) in stable distribution builds.
   - Never attach to game processes or bypass anti-cheat mechanisms.
   - Disruptive Windows shell tests must be conducted strictly in isolated virtual machines.
3. **Evidence Vocabulary**:
   - `NOT_STARTED`: No implementation exists.
   - `IMPLEMENTED_UNVERIFIED`: Code compiles but runtime behavior is not verified.
   - `PARTIALLY_VERIFIED`: Subset of acceptance criteria verified.
   - `VERIFIED`: All acceptance checks passed on a specific candidate commit and recorded environment.
   - `BLOCKED`: A technical constraint or external dependency blocks progress.
   - `EXPERIMENTAL`: Restricted to experimental opt-in builds.

## Pull Request Process

1. Create a focused branch off `main`.
2. Implement scoped changes with matching automated tests.
3. Verify formatting and linter compliance (`cargo fmt --check`, `cargo clippy`).
4. Execute tests locally and record evidence.
5. Submit a pull request with linked PRD requirements and verification status.
