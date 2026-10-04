//! Session cookie encoding, verification and cookie attributes.
//!
//! The format matches upstream's `base64url(json) + "." + base64url(hmac_sha256)`
//! in `_sailplane_auth`, so existing cookies keep working. Unlike upstream,
//! verification is constant-time and the cookie is marked `HttpOnly`.

use anyhow::Result;
use base64::Engine;
use serde::{Deserialize, Serialize};

/// Name of the session cookie.
pub const SESSION_COOKIE: &str = "_sailplane_auth";
/// Name of the short-lived OIDC transaction cookie.
pub const OIDC_STATE_COOKIE: &str = "__oidc_state";
/// Name of the colour-scheme preference cookie.
pub const COLOR_SCHEME_COOKIE: &str = "color_scheme";

const B64: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::URL_SAFE_NO_PAD;

/// The data carried inside the session cookie.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CookiePayload {
    /// Session row ID.
    pub sid: String,

    /// Raw Headscale API key for API-key sessions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Display-only profile snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<CookieProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CookieProfile {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
}

/// Encodes and signs a payload.
pub fn encode_cookie(payload: &CookiePayload, secret: &str) -> Result<String> {
    let json = serde_json::to_vec(payload)?;
    let signature = sign(&json, secret);
    Ok(format!("{}.{}", B64.encode(&json), B64.encode(signature)))
}

/// Verifies the signature and decodes the payload. Returns `None` when the
/// cookie is malformed or the signature does not match.
pub fn decode_cookie(value: &str, secret: &str) -> Option<CookiePayload> {
    let (body, signature) = value.split_once('.')?;
    let json = B64.decode(body).ok()?;
    let provided = B64.decode(signature).ok()?;

    let expected = sign(&json, secret);
    if !constant_time_eq(&provided, &expected) {
        return None;
    }

    serde_json::from_slice(&json).ok()
}

fn sign(json: &[u8], secret: &str) -> Vec<u8> {
    use hmac::Mac;
    let mut mac =
        hmac::Hmac::<sha2::Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(json);
    mac.finalize().into_bytes().to_vec()
}

/// Length-independent constant-time comparison.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// The OIDC transaction state stashed between `/oidc/start` and `/oidc/callback`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcTransaction {
    pub state: String,
    pub nonce: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verifier: Option<String>,
    pub redirect_uri: String,
}

impl OidcTransaction {
    pub fn encode(&self) -> Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    pub fn decode(raw: &str) -> Result<Self> {
        serde_json::from_str(raw).map_err(Into::into)
    }
}

/// Cookie attributes applied to the session cookie.
#[derive(Debug, Clone)]
pub struct CookieOptions {
    pub secure: bool,
    pub http_only: bool,
    pub max_age_seconds: i64,
    pub domain: Option<String>,
    pub path: String,
    pub same_site: SameSite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SameSite {
    /// Sent on top-level navigations; the only mode the session cookie needs.
    Lax,
}

impl SameSite {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lax => "Lax",
        }
    }
}

impl Default for CookieOptions {
    fn default() -> Self {
        Self {
            secure: true,
            http_only: true,
            max_age_seconds: 86400,
            domain: None,
            path: "/".into(),
            same_site: SameSite::Lax,
        }
    }
}

impl CookieOptions {
    /// Renders a `Set-Cookie` header value for the given cookie.
    pub fn render(&self, name: &str, value: &str) -> String {
        let mut out = format!("{name}={value}; Path={}; Max-Age={}", self.path, self.max_age_seconds);
        if self.http_only {
            out.push_str("; HttpOnly");
        }
        if self.secure {
            out.push_str("; Secure");
        }
        out.push_str(&format!("; SameSite={}", self.same_site.as_str()));
        if let Some(domain) = self.domain.as_deref() {
            out.push_str(&format!("; Domain={domain}"));
        }
        out
    }

    /// Renders a header that clears the cookie.
    pub fn render_cleared(&self, name: &str) -> String {
        let mut out = format!("{name}=; Path={}; Max-Age=0", self.path);
        if self.http_only {
            out.push_str("; HttpOnly");
        }
        if self.secure {
            out.push_str("; Secure");
        }
        out.push_str(&format!("; SameSite={}", self.same_site.as_str()));
        if let Some(domain) = self.domain.as_deref() {
            out.push_str(&format!("; Domain={domain}"));
        }
        out
    }
}

/// Extracts a cookie value from a `Cookie` header.
pub fn read_cookie(header: Option<&str>, name: &str) -> Option<String> {
    let header = header?;
    header.split(';').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        if key.trim() == name {
            Some(value.trim().to_string())
        } else {
            None
        }
    })
}

/// Validates a `returnTo`-style redirect target: must be a same-origin
/// absolute path.
pub fn safe_redirect(target: &str, fallback: &str) -> String {
    if target.starts_with('/') && !target.starts_with("//") && !target.contains('\\') {
        target.to_string()
    } else {
        fallback.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "0123456789012345678901234567890a";

    fn payload() -> CookiePayload {
        CookiePayload {
            sid: "session-1".into(),
            api_key: Some("hskey-api-secret".into()),
            profile: None,
        }
    }

    #[test]
    fn cookie_roundtrips() {
        let encoded = encode_cookie(&payload(), SECRET).unwrap();
        let decoded = decode_cookie(&encoded, SECRET).unwrap();
        assert_eq!(decoded.sid, "session-1");
        assert_eq!(decoded.api_key.as_deref(), Some("hskey-api-secret"));
    }

    #[test]
    fn tampered_payload_is_rejected() {
        let encoded = encode_cookie(&payload(), SECRET).unwrap();
        let (body, sig) = encoded.split_once('.').unwrap();

        // Re-encode a different payload with the original signature.
        let forged = CookiePayload {
            sid: "attacker".into(),
            api_key: None,
            profile: None,
        };
        let forged_body = B64.encode(serde_json::to_vec(&forged).unwrap());
        assert!(decode_cookie(&format!("{forged_body}.{sig}"), SECRET).is_none());

        // Garbage in the body slot.
        assert!(decode_cookie(&format!("{}.{sig}", "not-base64!!"), SECRET).is_none());
        let _ = body;
    }

    #[test]
    fn wrong_secret_is_rejected() {
        let encoded = encode_cookie(&payload(), SECRET).unwrap();
        assert!(decode_cookie(&encoded, "another-secret-another-secret12").is_none());
    }

    #[test]
    fn missing_signature_is_rejected() {
        assert!(decode_cookie("just-a-body", SECRET).is_none());
    }

    #[test]
    fn cookie_attributes_are_rendered() {
        let options = CookieOptions {
            secure: true,
            http_only: true,
            max_age_seconds: 3600,
            domain: Some("example.com".into()),
            path: "/admin".into(),
            same_site: SameSite::Lax,
        };
        let rendered = options.render("_sailplane_auth", "value");
        assert!(rendered.contains("Path=/admin"));
        assert!(rendered.contains("Max-Age=3600"));
        assert!(rendered.contains("HttpOnly"));
        assert!(rendered.contains("Secure"));
        assert!(rendered.contains("SameSite=Lax"));
        assert!(rendered.contains("Domain=example.com"));
    }

    #[test]
    fn cookie_header_parsing_ignores_surrounding_cookies() {
        let header = Some("a=1; _sailplane_auth=xyz; b=2");
        assert_eq!(read_cookie(header, "_sailplane_auth").as_deref(), Some("xyz"));
        assert_eq!(read_cookie(header, "missing"), None);
        assert_eq!(read_cookie(None, "_sailplane_auth"), None);
    }

    #[test]
    fn safe_redirect_rejects_cross_origin() {
        assert_eq!(safe_redirect("/machines", "/"), "/machines");
        assert_eq!(safe_redirect("//evil.com", "/"), "/");
        assert_eq!(safe_redirect("https://evil.com", "/"), "/");
        assert_eq!(safe_redirect("/\\evil", "/"), "/");
    }
}
