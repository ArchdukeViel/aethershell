/// Case-insensitively replaces all occurrences of `needle` with `replacement` in `haystack`.
fn replace_case_insensitive(haystack: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() {
        return haystack.to_string();
    }

    let mut result = String::with_capacity(haystack.len());
    let mut last_idx = 0;
    let needle_lower = needle.to_lowercase();
    let haystack_lower = haystack.to_lowercase();

    while let Some(rel_pos) = haystack_lower[last_idx..].find(&needle_lower) {
        let match_start = last_idx + rel_pos;
        let match_end = match_start + needle.len();

        result.push_str(&haystack[last_idx..match_start]);
        result.push_str(replacement);
        last_idx = match_end;
    }
    result.push_str(&haystack[last_idx..]);
    result
}

/// Sanitizes diagnostic strings by redacting username patterns, user paths, and sensitive tokens.
pub fn redact_sensitive_info(input: &str) -> String {
    let mut sanitized = input.to_string();

    // 1. Redact USERPROFILE before USERNAME to prevent partial replacement bugs (IVA-004)
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        if !userprofile.is_empty() {
            sanitized = replace_case_insensitive(&sanitized, &userprofile, "%USERPROFILE%");
        }
    }

    // 2. Redact USERNAME case-insensitively (IVA-004)
    if let Ok(username) = std::env::var("USERNAME") {
        if !username.is_empty() {
            sanitized = replace_case_insensitive(&sanitized, &username, "[REDACTED_USER]");
        }
    }

    // 3. Mask sensitive authentication tokens across all occurrences (IVA-001, IVA-004)
    let token_prefixes = [
        "gho_",        // GitHub OAuth
        "ghp_",        // GitHub Personal Access Token (classic)
        "github_pat_", // GitHub Fine-grained PAT
        "AIzaSy",      // Google / Gemini API key
        "Bearer ",     // Standard Authorization header
        "eyJ",         // JWT header prefix (base64 for {"alg":...)
    ];

    for prefix in token_prefixes {
        let mut search_idx = 0;
        while let Some(rel_pos) = sanitized[search_idx..].find(prefix) {
            let pos = search_idx + rel_pos;
            let start = pos + prefix.len();

            if sanitized[start..].starts_with("***") {
                search_idx = start + 3;
                continue;
            }

            let end = sanitized[start..]
                .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ',' || c == ';')
                .map(|p| start + p)
                .unwrap_or(sanitized.len());

            sanitized.replace_range(start..end, "***");
            search_idx = start + 3;
        }
    }

    sanitized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multiple_token_masking_iva_001() {
        let text = "Auth: ghp_token1 failed, secondary ghp_token2 active, and gho_oauth3";
        let redacted = redact_sensitive_info(text);
        assert!(!redacted.contains("token1"));
        assert!(!redacted.contains("token2"));
        assert!(!redacted.contains("oauth3"));
        assert!(redacted.contains("ghp_***"));
        assert!(redacted.contains("gho_***"));
    }

    #[test]
    fn test_expanded_token_prefixes_iva_004() {
        let text = "PAT: github_pat_11ABCD_secret, Key: AIzaSyD_secretKey, JWT: eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9";
        let redacted = redact_sensitive_info(text);
        assert!(!redacted.contains("11ABCD_secret"));
        assert!(!redacted.contains("secretKey"));
        assert!(!redacted.contains("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"));
        assert!(redacted.contains("github_pat_***"));
        assert!(redacted.contains("AIzaSy***"));
        assert!(redacted.contains("eyJ***"));
    }

    #[test]
    fn test_case_insensitive_path_redaction_iva_004() {
        let user = "TestUser";
        let mixed_path = "c:\\users\\testuser\\documents\\file.txt";
        let redacted = replace_case_insensitive(mixed_path, user, "[REDACTED_USER]");
        assert_eq!(redacted, "c:\\users\\[REDACTED_USER]\\documents\\file.txt");
    }
}
