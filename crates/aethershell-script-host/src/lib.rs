//! Supervised out-of-process Lua script host and trust verifier.
//!
//! Enforces explicit user digest trust (PRD Section 13.2) and crash recovery limits.

pub mod supervisor;
pub mod trust;

pub use supervisor::{ScriptSupervisor, SupervisorState};
pub use trust::{sha256_hex, verify_script_digest, verify_script_file_digest, TrustError};
