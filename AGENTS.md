# Engineering Conventions for Autonomous & Agentic Workflows

## Authority and Scope

- **Authoritative Baseline**: All work derives from the approved `PRD.md`.
- **Precedence Hierarchy**:
  1. Explicit current owner instructions.
  2. Approved owner decisions in `PRD.md` (Q01–Q41, Q09R).
  3. Non-negotiable safety and release constraints.
  4. Verified technical evidence and platform API contracts.
  5. Architecture Decision Records (ADRs).
  6. Existing codebase and tests.

## Rules of Engagement

1. **Anti-Hallucination & Evidence First**:
   - Every completed requirement must cite passing automated tests or recorded experimental evidence.
   - Use the approved status vocabulary: `NOT_STARTED`, `IMPLEMENTED_UNVERIFIED`, `PARTIALLY_VERIFIED`, `VERIFIED`, `BLOCKED`, `NOT_SUPPORTED`, `EXPERIMENTAL`.
2. **Windows & PowerShell Conventions**:
   - Assume Windows OS and PowerShell (no Unix bashisms).
   - Use BOM-free UTF-8 for JSON, TOML, and code files.
   - Do not discard uncommitted work.
3. **Safety Boundaries (Q37:A)**:
   - Disruptive Windows shell tests (Explorer injection, taskbar hook replacement, system taskbar mutation, GPU resets) must run in isolated VMs only.
   - Never inject into or terminate `explorer.exe` on daily-use physical host machines.
4. **Independent Review Requirement**:
   - Implementation work must be audited by an independent reviewer or specialized subagent before claiming acceptance.
