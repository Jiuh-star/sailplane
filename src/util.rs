//! Shared helpers for timestamps, random tokens, and input validation.

use chrono::{DateTime, Utc};

/// Parses an RFC 3339 timestamp as emitted by Headscale.
///
/// Headscale uses Go's zero time (`0001-01-01T00:00:00Z`) to mean "never";
/// it parses as far in the past.
pub fn parse_rfc3339(input: &str) -> Option<DateTime<Utc>> {
    if input.is_empty() {
        return None;
    }
    DateTime::parse_from_rfc3339(input)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// Formats a timestamp for the API as UTC with millisecond precision.
pub fn format_rfc3339(dt: DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Returns a random URL-safe token with `bytes` bytes of entropy.
/// Used for session IDs, nonces, and PKCE verifiers.
pub fn random_token(bytes: usize) -> String {
    use base64::Engine;
    use rand::RngCore;

    let mut buf = vec![0u8; bytes];
    rand::rng().fill_bytes(&mut buf);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

/// Returns the hex-encoded SHA-256 digest.
pub fn sha256_hex(input: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(input);
    hex::encode(hasher.finalize())
}

/// Returns the base64url-encoded SHA-256 digest, without padding.
/// Used for PKCE `code_challenge` values.
pub fn sha256_hex_b64url(input: &[u8]) -> String {
    use base64::Engine;
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(input);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hasher.finalize())
}

/// Returns true when `s` is a valid DNS label for a machine or host name.
pub fn is_valid_dns_label(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 63
        && !s.starts_with('-')
        && !s.ends_with('-')
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Returns true when `s` is a plausible domain name.
/// Sailplane writes values that pass this check to the Headscale config file,
/// so control characters cannot reach a YAML line.
pub fn is_valid_dns_name(s: &str) -> bool {
    let name = s.strip_suffix('.').unwrap_or(s);
    !name.is_empty()
        && name.len() <= 253
        && name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
}

/// Returns `s` truncated to `max` characters, with an ellipsis when cut.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_time_parses_and_is_in_the_past() {
        let parsed = parse_rfc3339("0001-01-01T00:00:00Z").unwrap();
        assert!(parsed < Utc::now());
    }

    #[test]
    fn empty_timestamp_is_none() {
        assert!(parse_rfc3339("").is_none());
    }

    #[test]
    fn dns_label_rules() {
        assert!(is_valid_dns_label("my-machine1"));
        assert!(!is_valid_dns_label("-bad"));
        assert!(!is_valid_dns_label("Bad"));
        assert!(!is_valid_dns_label(""));
    }

    #[test]
    fn dns_names_reject_line_breaks_and_structure() {
        assert!(is_valid_dns_name("example.com"));
        assert!(is_valid_dns_name("corp.example.com."));
        assert!(is_valid_dns_name("git"));

        // The shapes that would rewrite a YAML document.
        assert!(!is_valid_dns_name("example.com\nfoo: bar"));
        assert!(!is_valid_dns_name("example.com\rfoo"));
        assert!(!is_valid_dns_name("a b"));
        assert!(!is_valid_dns_name("a/b"));
        assert!(!is_valid_dns_name(".."));
        assert!(!is_valid_dns_name(""));
    }

    #[test]
    fn random_tokens_are_unique_and_url_safe() {
        let a = random_token(16);
        let b = random_token(16);
        assert_ne!(a, b);
        assert!(!a.contains('+') && !a.contains('/') && !a.contains('='));
    }
}
