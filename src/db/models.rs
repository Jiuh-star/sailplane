//! Row types for the Sailplane database.

use chrono::{DateTime, TimeZone, Utc};
use rusqlite::Row;
use serde::{Deserialize, Serialize};

use crate::auth::roles::Role;

/// A Sailplane account. OIDC and proxy-auth users get local accounts.
/// API-key logins are session-only and never stored here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SailplaneUser {
    pub id: String,
    /// Stable identity: the OIDC subject, or `proxy:<header value>`.
    pub sub: String,
    pub name: Option<String>,
    pub email: Option<String>,
    pub picture: Option<String>,
    pub role: Role,
    /// The Headscale user this account is linked to.
    pub headscale_user_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_login_at: Option<DateTime<Utc>>,
}

impl SailplaneUser {
    pub fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            sub: row.get("sub")?,
            name: row.get("name")?,
            email: row.get("email")?,
            picture: row.get("picture")?,
            role: Role::parse(&row.get::<_, String>("role")?),
            headscale_user_id: row.get("headscale_user_id")?,
            created_at: millis_to_datetime(row.get("created_at")?),
            updated_at: millis_to_datetime(row.get("updated_at")?),
            last_login_at: row
                .get::<_, Option<i64>>("last_login_at")?
                .map(millis_to_datetime),
        })
    }

    /// Label shown in the UI: display name, then email, then the raw subject.
    pub fn label(&self) -> &str {
        self.name
            .as_deref()
            .filter(|n| !n.trim().is_empty())
            .or(self.email.as_deref())
            .unwrap_or(&self.sub)
    }

    pub fn is_owner(&self) -> bool {
        self.role == Role::Owner
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    Oidc,
    ApiKey,
}

impl SessionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Oidc => "oidc",
            Self::ApiKey => "api_key",
        }
    }

    pub fn parse(input: &str) -> Self {
        match input {
            "api_key" => Self::ApiKey,
            _ => Self::Oidc,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub kind: SessionKind,
    /// Set for OIDC sessions.
    pub user_id: Option<String>,
    /// SHA-256 of the API key, set for API-key sessions.
    pub api_key_hash: Option<String>,
    /// Display-only prefix shown in the UI.
    pub api_key_display: Option<String>,
    /// Stored only when `oidc.use_end_session` is enabled.
    pub oidc_id_token: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

impl Session {
    pub fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            kind: SessionKind::parse(&row.get::<_, String>("kind")?),
            user_id: row.get("user_id")?,
            api_key_hash: row.get("api_key_hash")?,
            api_key_display: row.get("api_key_display")?,
            oidc_id_token: row.get("oidc_id_token")?,
            expires_at: millis_to_datetime(row.get("expires_at")?),
            created_at: millis_to_datetime(row.get("created_at")?),
        })
    }

    pub fn is_expired(&self) -> bool {
        self.expires_at < Utc::now()
    }
}
/// One state-changing request, as Sailplane saw it.
///
/// Headscale keeps no history, so the audit log is the only record of who
/// changed what.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: i64,
    pub at: DateTime<Utc>,
    /// Account label, or the API key prefix for key-authenticated sessions.
    pub actor: String,
    pub role: String,
    pub method: String,
    pub path: String,
    pub status: u16,
    /// Why it failed, when it did.
    pub detail: Option<String>,
}

impl AuditEntry {
    pub fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            at: millis_to_datetime(row.get("at")?),
            actor: row.get("actor")?,
            role: row.get("role")?,
            method: row.get("method")?,
            path: row.get("path")?,
            status: row.get("status")?,
            detail: row.get("detail")?,
        })
    }
}

/// Converts a stored millisecond timestamp into a UTC datetime.
///
/// Values outside the representable range fall back to the Unix epoch, so a
/// corrupt row cannot panic and an expired session fails closed.
pub fn millis_to_datetime(millis: i64) -> DateTime<Utc> {
    Utc.timestamp_millis_opt(millis)
        .single()
        .unwrap_or(DateTime::<Utc>::UNIX_EPOCH)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_of_range_timestamps_fall_back_to_the_epoch() {
        let valid = 1_700_000_000_000;
        assert_eq!(millis_to_datetime(valid).timestamp_millis(), valid);
        assert_eq!(millis_to_datetime(i64::MAX), DateTime::<Utc>::UNIX_EPOCH);
    }

    #[test]
    fn session_kind_roundtrips() {
        assert_eq!(SessionKind::parse("api_key"), SessionKind::ApiKey);
        assert_eq!(SessionKind::parse("oidc"), SessionKind::Oidc);
        assert_eq!(SessionKind::parse("nonsense"), SessionKind::Oidc);
    }

    #[test]
    fn labels_fall_back_through_available_fields() {
        let mut user = SailplaneUser {
            id: "1".into(),
            sub: "subject".into(),
            name: Some("  ".into()),
            email: Some("a@b.c".into()),
            picture: None,
            role: Role::Member,
            headscale_user_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_login_at: None,
        };
        assert_eq!(user.label(), "a@b.c");

        user.email = None;
        assert_eq!(user.label(), "subject");
    }
}
