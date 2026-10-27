use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TrustError {
    #[error(
        "Script digest mismatch! Expected: {expected}, Computed: {actual}. Execution blocked."
    )]
    DigestMismatch { expected: String, actual: String },
    #[error("Script has not been granted explicit user trust.")]
    Untrusted,
    #[error("Failed to read script file: {0}")]
    IoError(String),
    #[error("Script path '{0}' is invalid, malformed, or does not exist.")]
    InvalidPath(String),
    #[error("Conflicting trust records discovered for script '{0}'.")]
    ConflictingTrustRecords(String),
    #[error("Malformed trust entry for script '{0}': {1}")]
    MalformedTrustEntry(String, String),
}

/// Normalizes a filesystem path by canonicalizing existing files and stripping verbatim UNC prefixes (`\\?\`).
pub fn normalize_script_path(path: &Path) -> Result<PathBuf, std::io::Error> {
    if !path.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Path does not exist: {}", path.display()),
        ));
    }
    let canonical = std::fs::canonicalize(path)?;
    let s = canonical.to_string_lossy();
    if let Some(stripped) = s.strip_prefix(r"\\?\") {
        Ok(PathBuf::from(stripped))
    } else {
        Ok(canonical)
    }
}

/// Compares two filesystem paths for equality, case-insensitively on Windows.
pub fn paths_equal(p1: &Path, p2: &Path) -> bool {
    let s1 = p1.to_string_lossy();
    let s2 = p2.to_string_lossy();
    s1.eq_ignore_ascii_case(&s2)
}

/// Authorizes a target script package against an explicit trust registry (PRD Section 13.2 / Gate G10).
///
/// Ensures:
/// 1. Path identity identifies the approved record (never a bare filename or arbitrary name match).
/// 2. Paths are normalized and validated to prevent ambiguous or conflicting trust entries.
/// 3. Script content digest matches the approved digest with strict cryptographic equality.
/// 4. Conflicting or malformed trust entries fail closed.
/// 5. Distinct paths with identical filenames do not inherit each other's approval.
///
/// Returns the verified approved SHA-256 digest on success, or a `TrustError` on failure.
pub fn authorize_script_trust(
    script_path: &Path,
    configured_path_str: &str,
    trusted_digests: &HashMap<String, String>,
) -> Result<String, TrustError> {
    if !script_path.is_file() {
        return Err(TrustError::InvalidPath(script_path.display().to_string()));
    }

    let canonical_script = normalize_script_path(script_path)
        .map_err(|e| TrustError::IoError(format!("Path normalization failed: {}", e)))?;

    // Read script bytes and compute actual SHA-256 digest
    let script_bytes = std::fs::read(&canonical_script)
        .map_err(|e| TrustError::IoError(format!("Failed to read script file: {}", e)))?;
    let actual_digest = sha256_hex(&script_bytes);

    // Find all matching trust entries for this canonical path or exact configured path string
    let mut matching_approvals: Vec<(&str, &str)> = Vec::new();

    for (k, v) in trusted_digests {
        let k_trimmed = k.trim();
        let v_trimmed = v.trim();

        if k_trimmed.is_empty() {
            return Err(TrustError::MalformedTrustEntry(
                k.clone(),
                "Empty path key in trust ledger".into(),
            ));
        }

        if v_trimmed.len() != 64 || !v_trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(TrustError::MalformedTrustEntry(
                k.clone(),
                format!(
                    "Digest must be a 64-character hexadecimal SHA-256 string (got length {})",
                    v_trimmed.len()
                ),
            ));
        }

        // Determine if key 'k' identifies this script:
        // A. Exact string match against configured_path_str (case-insensitive on Windows)
        let exact_configured_match = !configured_path_str.trim().is_empty()
            && k_trimmed.eq_ignore_ascii_case(configured_path_str.trim());

        // B. Canonicalized path match:
        let canonical_match = {
            let key_path = Path::new(k_trimmed);
            if key_path.exists() {
                if let Ok(canon_key) = normalize_script_path(key_path) {
                    paths_equal(&canon_key, &canonical_script)
                } else {
                    false
                }
            } else {
                paths_equal(key_path, &canonical_script) || paths_equal(key_path, script_path)
            }
        };

        if exact_configured_match || canonical_match {
            matching_approvals.push((k_trimmed, v_trimmed));
        }
    }

    if matching_approvals.is_empty() {
        return Err(TrustError::Untrusted);
    }

    // Check for conflicting approved digests for this file
    let first_digest = matching_approvals[0].1;
    for &(_k, d) in &matching_approvals[1..] {
        if !d.eq_ignore_ascii_case(first_digest) {
            return Err(TrustError::ConflictingTrustRecords(
                canonical_script.display().to_string(),
            ));
        }
    }

    let approved_digest = first_digest.to_lowercase();

    // Enforce strict digest equality
    if !actual_digest.eq_ignore_ascii_case(&approved_digest) {
        return Err(TrustError::DigestMismatch {
            expected: approved_digest,
            actual: actual_digest,
        });
    }

    Ok(approved_digest)
}

/// Computes a standard SHA-256 hexadecimal hash over bytes without external dependencies.
#[allow(clippy::chunks_exact_to_as_chunks)]
pub fn sha256_hex(data: &[u8]) -> String {
    let mut h = [
        0x6a09e667u32,
        0xbb67ae85u32,
        0x3c6ef372u32,
        0xa54ff53au32,
        0x510e527fu32,
        0x9b05688cu32,
        0x1f83d9abu32,
        0x5be0cd19u32,
    ];

    let k = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let bit_len = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);

    while (msg.len() % 64) != 56 {
        msg.push(0x00);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, part) in chunk.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([part[0], part[1], part[2], part[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut h_val = h[7];

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h_val
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(k[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h_val = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(h_val);
    }

    let mut out = String::with_capacity(64);
    for val in h {
        out.push_str(&format!("{:08x}", val));
    }
    out
}

/// Verifies that script code matches an explicitly approved SHA-256 package digest (PRD Section 13.2 / Gate G10).
pub fn verify_script_digest(script_bytes: &[u8], approved_digest: &str) -> Result<(), TrustError> {
    if approved_digest.trim().is_empty() {
        return Err(TrustError::Untrusted);
    }

    let actual_digest = sha256_hex(script_bytes);
    if !actual_digest.eq_ignore_ascii_case(approved_digest.trim()) {
        return Err(TrustError::DigestMismatch {
            expected: approved_digest.to_string(),
            actual: actual_digest,
        });
    }

    Ok(())
}

/// Reads a script file from disk and verifies its SHA-256 digest matches approved digest.
pub fn verify_script_file_digest(
    path: &std::path::Path,
    approved_digest: &str,
) -> Result<(), TrustError> {
    let bytes = std::fs::read(path).map_err(|e| TrustError::IoError(e.to_string()))?;
    verify_script_digest(&bytes, approved_digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_known_vector() {
        // Known SHA-256 of empty string: e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );

        // Known SHA-256 of "hello"
        assert_eq!(
            sha256_hex(b"hello"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn test_script_digest_trust_verification() {
        let script = b"print('Hello from trusted script')";
        let valid_digest = sha256_hex(script);

        // Matching digest succeeds
        assert!(verify_script_digest(script, &valid_digest).is_ok());

        // Altered script fails with DigestMismatch
        let tampered = b"print('Tampered script!')";
        let res = verify_script_digest(tampered, &valid_digest);
        assert!(matches!(res, Err(TrustError::DigestMismatch { .. })));

        // Empty approval fails with Untrusted
        let res2 = verify_script_digest(script, "");
        assert_eq!(res2, Err(TrustError::Untrusted));
    }

    #[test]
    fn test_authorize_script_trust_lifecycle() {
        let temp_dir = std::env::temp_dir().join(format!(
            "test_trust_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let dir_a = temp_dir.join("dir_a");
        let dir_b = temp_dir.join("dir_b");
        std::fs::create_dir_all(&dir_a).unwrap();
        std::fs::create_dir_all(&dir_b).unwrap();

        let script_a = dir_a.join("widget.lua");
        let script_b = dir_b.join("widget.lua");

        let content_a = "function init() print('A') end";
        let content_b = "function init() print('B') end";
        std::fs::write(&script_a, content_a).unwrap();
        std::fs::write(&script_b, content_b).unwrap();

        let digest_a = sha256_hex(content_a.as_bytes());
        let digest_b = sha256_hex(content_b.as_bytes());

        let mut trust_registry = HashMap::new();
        trust_registry.insert(script_a.to_string_lossy().to_string(), digest_a.clone());

        // 1. Script A with matching digest succeeds
        let auth_res =
            authorize_script_trust(&script_a, &script_a.to_string_lossy(), &trust_registry);
        assert_eq!(auth_res, Ok(digest_a.clone()));

        // 2. Modified Script A with same path fails with DigestMismatch
        std::fs::write(&script_a, "function init() print('A MODIFIED') end").unwrap();
        let modified_res =
            authorize_script_trust(&script_a, &script_a.to_string_lossy(), &trust_registry);
        assert!(matches!(
            modified_res,
            Err(TrustError::DigestMismatch { .. })
        ));

        // Restore Script A
        std::fs::write(&script_a, content_a).unwrap();

        // 3. Script B (same filename 'widget.lua', distinct path) does NOT inherit Script A's approval
        let script_b_res =
            authorize_script_trust(&script_b, &script_b.to_string_lossy(), &trust_registry);
        assert_eq!(script_b_res, Err(TrustError::Untrusted));

        // 4. Bare filename entry in registry does not authorize script in subdirectory
        let mut bare_registry = HashMap::new();
        bare_registry.insert("widget.lua".into(), digest_a.clone());
        let bare_res =
            authorize_script_trust(&script_a, &script_a.to_string_lossy(), &bare_registry);
        assert_eq!(bare_res, Err(TrustError::Untrusted));

        // 5. Conflicting digests fail closed
        let mut conflicting_registry = HashMap::new();
        conflicting_registry.insert(script_a.to_string_lossy().to_string(), digest_a.clone());
        let canon_a = normalize_script_path(&script_a).unwrap();
        conflicting_registry.insert(
            canon_a.to_string_lossy().to_string(),
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".into(),
        );
        let conflict_res = authorize_script_trust(
            &script_a,
            &script_a.to_string_lossy(),
            &conflicting_registry,
        );
        assert!(matches!(
            conflict_res,
            Err(TrustError::ConflictingTrustRecords(..))
        ));

        // 6. Malformed digest fails closed
        let mut malformed_registry = HashMap::new();
        malformed_registry.insert(
            script_a.to_string_lossy().to_string(),
            "bad_short_digest".into(),
        );
        let malformed_res =
            authorize_script_trust(&script_a, &script_a.to_string_lossy(), &malformed_registry);
        assert!(matches!(
            malformed_res,
            Err(TrustError::MalformedTrustEntry(..))
        ));

        // 7. Explicit reauthorization of Script B allows Script B to run
        let mut registry_with_b = HashMap::new();
        registry_with_b.insert(script_b.to_string_lossy().to_string(), digest_b.clone());
        let reauth_b =
            authorize_script_trust(&script_b, &script_b.to_string_lossy(), &registry_with_b);
        assert_eq!(reauth_b, Ok(digest_b));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
