//! SQLite persistence for Sailplane accounts, sessions, and agent host info.
//!
//! Mirrors the upstream Drizzle schema, created idempotently and versioned with
//! SQLite's `user_version` pragma.

pub mod models;

use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};

pub use models::{AuditEntry, SailplaneUser, Session, SessionKind};

use crate::auth::roles::Role;

/// Current schema version; incremented when a migration is appended.
const SCHEMA_VERSION: i64 = 2;

/// Maximum audit rows kept; the log records recent history only.
const AUDIT_LIMIT: i64 = 5000;

/// Handle to the Sailplane database. Cloning shares the single connection.
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    /// Opens (creating if needed) the database at `<data_path>/sailplane_persist.db`.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("failed to create data directory {}", parent.display())
            })?;
        }

        let conn = Connection::open(path)
            .with_context(|| format!("failed to open database at {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        // Keep the page cache small: this is an admin UI, not a busy service.
        conn.pragma_update(None, "cache_size", -2000)?;

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.migrate()?;
        Ok(db)
    }

    /// Test-only view of the host-info table.
    #[cfg(test)]
    pub fn list_host_info(&self) -> Result<Vec<(String, String)>> {
        self.with(|conn| {
            let mut stmt = conn.prepare("SELECT host_id, payload FROM host_info")?;
            let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .context("failed to list host info")
        })
    }

    /// In-memory database for tests; production always uses [`Db::open`].
    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock().expect("db lock poisoned");
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;

        if version < 1 {
            conn.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS users (
                    id                TEXT PRIMARY KEY,
                    sub               TEXT NOT NULL UNIQUE,
                    name              TEXT,
                    email             TEXT,
                    picture           TEXT,
                    role              TEXT NOT NULL DEFAULT 'member',
                    headscale_user_id TEXT UNIQUE,
                    created_at        INTEGER NOT NULL,
                    updated_at        INTEGER NOT NULL,
                    last_login_at     INTEGER
                );
                CREATE UNIQUE INDEX IF NOT EXISTS users_sub_unique ON users (sub);

                CREATE TABLE IF NOT EXISTS auth_sessions (
                    id              TEXT PRIMARY KEY,
                    kind            TEXT NOT NULL,
                    user_id         TEXT REFERENCES users (id) ON DELETE CASCADE,
                    api_key_hash    TEXT,
                    api_key_display TEXT,
                    oidc_id_token   TEXT,
                    expires_at      INTEGER NOT NULL,
                    created_at      INTEGER NOT NULL
                );
                CREATE INDEX IF NOT EXISTS auth_sessions_expires_at
                    ON auth_sessions (expires_at);

                CREATE TABLE IF NOT EXISTS host_info (
                    host_id    TEXT PRIMARY KEY,
                    payload    TEXT NOT NULL,
                    updated_at INTEGER NOT NULL
                );
                "#,
            )?;
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }

        if version < 2 {
            // Headscale keeps no record of who changed what, so Sailplane writes
            // one row per state-changing request.
            conn.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS audit_log (
                    id     INTEGER PRIMARY KEY AUTOINCREMENT,
                    at     INTEGER NOT NULL,
                    actor  TEXT NOT NULL,
                    role   TEXT NOT NULL,
                    method TEXT NOT NULL,
                    path   TEXT NOT NULL,
                    status INTEGER NOT NULL,
                    detail TEXT
                );
                CREATE INDEX IF NOT EXISTS audit_log_at ON audit_log (at DESC);
                "#,
            )?;
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }

        Ok(())
    }

    /// Runs a closure with the connection held. Synchronous; async callers must
    /// use [`Db::run`].
    pub fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let conn = self.conn.lock().expect("db lock poisoned");
        f(&conn)
    }

    /// Runs the closure on Tokio's blocking pool.
    pub async fn run<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        let this = self.clone();
        tokio::task::spawn_blocking(move || this.with(f))
            .await
            .context("database task panicked")?
    }

    // --- Users ---

    pub fn get_user(&self, id: &str) -> Result<Option<SailplaneUser>> {
        self.with(|conn| {
            conn.query_row(
                "SELECT * FROM users WHERE id = ?1",
                params![id],
                SailplaneUser::from_row,
            )
            .optional()
            .context("failed to query user")
        })
    }

    pub fn count_users(&self) -> Result<i64> {
        self.with(|conn| {
            conn.query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))
                .context("failed to count users")
        })
    }

    /// Inserts a user. The first user ever created becomes the owner.
    pub fn create_user(
        &self,
        sub: &str,
        name: Option<&str>,
        email: Option<&str>,
        picture: Option<&str>,
        initial_role: Role,
    ) -> Result<SailplaneUser> {
        let now = Utc::now().timestamp_millis();
        let id = uuid::Uuid::new_v4().simple().to_string();

        let role = if self.count_users()? == 0 {
            Role::Owner
        } else if initial_role == Role::Owner {
            // Owner is never grantable through login.
            Role::Member
        } else {
            initial_role
        };

        self.with(|conn| {
            conn.execute(
                "INSERT INTO users (id, sub, name, email, picture, role, created_at, updated_at, last_login_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, ?7)",
                params![id, sub, name, email, picture, role.as_str(), now],
            )
            .context("failed to insert user")?;
            Ok(())
        })?;

        self.get_user(&id)?
            .context("user vanished immediately after insert")
    }

    pub fn update_user_profile(
        &self,
        id: &str,
        name: Option<&str>,
        email: Option<&str>,
        picture: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now().timestamp_millis();
        self.with(|conn| {
            conn.execute(
                "UPDATE users
                    SET name = COALESCE(?2, name),
                        email = COALESCE(?3, email),
                        picture = COALESCE(?4, picture),
                        updated_at = ?5,
                        last_login_at = ?5
                  WHERE id = ?1",
                params![id, name, email, picture, now],
            )
            .context("failed to update user profile")?;
            Ok(())
        })
    }

    /// Updates a non-owner user's role. Ownership moves only through
    /// [`Db::transfer_ownership`], so a request for `Role::Owner` is ignored.
    pub fn set_user_role(&self, id: &str, role: Role) -> Result<()> {
        if role == Role::Owner {
            return Ok(());
        }
        let now = Utc::now().timestamp_millis();
        self.with(|conn| {
            conn.execute(
                "UPDATE users SET role = ?2, updated_at = ?3
                  WHERE id = ?1 AND role != 'owner'",
                params![id, role.as_str(), now],
            )
            .context("failed to set user role")?;
            Ok(())
        })
    }

    /// Links a Sailplane account to a Headscale user. Fails when that Headscale
    /// user is already linked to another account.
    pub fn link_headscale_user(&self, id: &str, headscale_user_id: &str) -> Result<()> {
        let now = Utc::now().timestamp_millis();
        self.with(|conn| {
            let existing: Option<String> = conn
                .query_row(
                    "SELECT id FROM users WHERE headscale_user_id = ?1",
                    params![headscale_user_id],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(existing) = existing {
                if existing != id {
                    anyhow::bail!("that Headscale user is already linked to another account");
                }
                return Ok(());
            }

            conn.execute(
                "UPDATE users SET headscale_user_id = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, headscale_user_id, now],
            )
            .context("failed to link headscale user")?;
            Ok(())
        })
    }
    /// Demotes the current owner to admin and promotes `id` to owner.
    pub fn transfer_ownership(&self, id: &str) -> Result<()> {
        let now = Utc::now().timestamp_millis();
        self.with(|conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute(
                "UPDATE users SET role = 'admin', updated_at = ?1 WHERE role = 'owner'",
                params![now],
            )?;
            tx.execute(
                "UPDATE users SET role = 'owner', updated_at = ?1 WHERE id = ?2",
                params![now, id],
            )?;
            tx.commit()?;
            Ok(())
        })
    }

    pub fn delete_user(&self, id: &str) -> Result<()> {
        self.with(|conn| {
            conn.execute("DELETE FROM users WHERE id = ?1", params![id])
                .context("failed to delete user")?;
            Ok(())
        })
    }
    // --- Sessions ---

    pub fn create_oidc_session(
        &self,
        user_id: &str,
        id_token: Option<&str>,
        ttl: chrono::Duration,
    ) -> Result<Session> {
        self.create_session(SessionKind::Oidc, Some(user_id), None, None, id_token, ttl)
    }

    pub fn create_api_key_session(
        &self,
        api_key: &str,
        display: &str,
        ttl: chrono::Duration,
    ) -> Result<Session> {
        let hash = crate::util::sha256_hex(api_key.as_bytes());
        self.create_session(
            SessionKind::ApiKey,
            None,
            Some(&hash),
            Some(display),
            None,
            ttl,
        )
    }

    fn create_session(
        &self,
        kind: SessionKind,
        user_id: Option<&str>,
        api_key_hash: Option<&str>,
        api_key_display: Option<&str>,
        id_token: Option<&str>,
        ttl: chrono::Duration,
    ) -> Result<Session> {
        let now = Utc::now();
        let id = uuid::Uuid::new_v4().simple().to_string();
        let expires_at = (now + ttl).timestamp_millis();

        self.with(|conn| {
            conn.execute(
                "INSERT INTO auth_sessions
                     (id, kind, user_id, api_key_hash, api_key_display, oidc_id_token, expires_at, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    id,
                    kind.as_str(),
                    user_id,
                    api_key_hash,
                    api_key_display,
                    id_token,
                    expires_at,
                    now.timestamp_millis()
                ],
            )
            .context("failed to insert session")?;
            Ok(())
        })?;

        self.get_session(&id)?
            .context("session vanished immediately after insert")
    }

    pub fn get_session(&self, id: &str) -> Result<Option<Session>> {
        self.with(|conn| {
            conn.query_row(
                "SELECT * FROM auth_sessions WHERE id = ?1",
                params![id],
                Session::from_row,
            )
            .optional()
            .context("failed to query session")
        })
    }

    pub fn delete_session(&self, id: &str) -> Result<()> {
        self.with(|conn| {
            conn.execute("DELETE FROM auth_sessions WHERE id = ?1", params![id])
                .context("failed to delete session")?;
            Ok(())
        })
    }

    pub fn prune_expired_sessions(&self) -> Result<usize> {
        let now = Utc::now().timestamp_millis();
        self.with(|conn| {
            let removed = conn
                .execute("DELETE FROM auth_sessions WHERE expires_at < ?1", params![now])
                .context("failed to prune sessions")?;
            Ok(removed)
        })
    }

    // --- Audit log ---

    /// Appends one entry and trims the oldest rows beyond [`AUDIT_LIMIT`].
    pub fn record_audit(&self, entry: &AuditEntry) -> Result<()> {
        self.with(|conn| {
            conn.execute(
                "INSERT INTO audit_log (at, actor, role, method, path, status, detail)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    entry.at.timestamp_millis(),
                    entry.actor,
                    entry.role,
                    entry.method,
                    entry.path,
                    entry.status,
                    entry.detail,
                ],
            )
            .context("failed to record an audit entry")?;

            conn.execute(
                "DELETE FROM audit_log WHERE id NOT IN
                   (SELECT id FROM audit_log ORDER BY id DESC LIMIT ?1)",
                params![AUDIT_LIMIT],
            )
            .context("failed to trim the audit log")?;
            Ok(())
        })
    }

    /// Returns the most recent entries, newest first.
    pub fn list_audit(&self, limit: i64, before_id: Option<i64>) -> Result<Vec<AuditEntry>> {
        self.with(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, at, actor, role, method, path, status, detail
                   FROM audit_log
                  WHERE (?1 IS NULL OR id < ?1)
                  ORDER BY id DESC
                  LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![before_id, limit], AuditEntry::from_row)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .context("failed to read the audit log")
        })
    }

    // --- Host info ---

    pub fn upsert_host_info(&self, host_id: &str, payload: &str) -> Result<()> {
        let now = Utc::now().timestamp_millis();
        self.with(|conn| {
            conn.execute(
                "INSERT INTO host_info (host_id, payload, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT (host_id) DO UPDATE SET payload = ?2, updated_at = ?3",
                params![host_id, payload, now],
            )
            .context("failed to upsert host info")?;
            Ok(())
        })
    }

    /// Removes host-info rows whose host ID is not in `keep`.
    ///
    /// An empty `keep` list means the caller has no node snapshot. The call is
    /// then a no-op, so a failed poll cannot wipe the table.
    pub fn prune_host_info(&self, keep: &[String]) -> Result<usize> {
        if keep.is_empty() {
            return Ok(0);
        }

        self.with(|conn| {
            let placeholders = (1..=keep.len())
                .map(|i| format!("?{i}"))
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!("DELETE FROM host_info WHERE host_id NOT IN ({placeholders})");
            let params = rusqlite::params_from_iter(keep.iter());
            conn.execute(&sql, params)
                .context("failed to prune host info")
        })
    }
}

/// Lists every Sailplane account, oldest first.
pub fn list_users(conn: &Connection) -> Result<Vec<SailplaneUser>> {
    let mut stmt = conn.prepare("SELECT * FROM users ORDER BY created_at ASC")?;
    let rows = stmt.query_map([], SailplaneUser::from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .context("failed to list users")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_user_becomes_owner() {
        let db = Db::open_in_memory().unwrap();
        let first = db
            .create_user("sub-1", Some("A"), None, None, Role::Member)
            .unwrap();
        assert_eq!(first.role, Role::Owner);

        let second = db
            .create_user("sub-2", Some("B"), None, None, Role::Admin)
            .unwrap();
        assert_eq!(second.role, Role::Admin);
    }

    #[test]
    fn owner_is_never_granted_at_creation() {
        let db = Db::open_in_memory().unwrap();
        db.create_user("sub-1", None, None, None, Role::Member).unwrap();
        let second = db
            .create_user("sub-2", None, None, None, Role::Owner)
            .unwrap();
        assert_eq!(second.role, Role::Member);
    }

    #[test]
    fn linking_refuses_a_claimed_headscale_user() {
        let db = Db::open_in_memory().unwrap();
        let a = db.create_user("a", None, None, None, Role::Member).unwrap();
        let b = db.create_user("b", None, None, None, Role::Member).unwrap();

        db.link_headscale_user(&a.id, "hs-1").unwrap();
        assert!(db.link_headscale_user(&b.id, "hs-1").is_err());
        // Re-linking to the same account is a no-op.
        db.link_headscale_user(&a.id, "hs-1").unwrap();
    }

    #[test]
    fn transferring_ownership_demotes_the_previous_owner() {
        let db = Db::open_in_memory().unwrap();
        let a = db.create_user("a", None, None, None, Role::Member).unwrap();
        let b = db.create_user("b", None, None, None, Role::Member).unwrap();
        assert_eq!(a.role, Role::Owner);

        db.transfer_ownership(&b.id).unwrap();
        assert_eq!(db.get_user(&a.id).unwrap().unwrap().role, Role::Admin);
        assert_eq!(db.get_user(&b.id).unwrap().unwrap().role, Role::Owner);
    }

    #[test]
    fn expired_sessions_are_pruned() {
        let db = Db::open_in_memory().unwrap();
        let user = db.create_user("sub", None, None, None, Role::Member).unwrap();
        // Negative TTL: already expired.
        let session = db
            .create_oidc_session(&user.id, None, chrono::Duration::seconds(-10))
            .unwrap();
        assert_eq!(db.prune_expired_sessions().unwrap(), 1);
        assert!(db.get_session(&session.id).unwrap().is_none());
    }

    #[test]
    fn deleting_a_user_cascades_to_sessions() {
        let db = Db::open_in_memory().unwrap();
        let user = db.create_user("sub", None, None, None, Role::Member).unwrap();
        let session = db
            .create_oidc_session(&user.id, None, chrono::Duration::seconds(3600))
            .unwrap();

        db.delete_user(&user.id).unwrap();
        assert!(db.get_session(&session.id).unwrap().is_none());
    }

    #[test]
    fn host_info_is_upserted_and_pruned() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_host_info("nodekey:1", r#"{"IPNVersion":"1.2.3"}"#).unwrap();
        db.upsert_host_info("nodekey:2", r#"{"IPNVersion":"1.2.4"}"#).unwrap();
        db.upsert_host_info("nodekey:1", r#"{"IPNVersion":"1.2.5"}"#).unwrap();

        let rows = db.list_host_info().unwrap();
        assert_eq!(rows.len(), 2);

        let pruned = db.prune_host_info(&["nodekey:2".to_string()]).unwrap();
        assert_eq!(pruned, 1);

        let rows = db.list_host_info().unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].1.contains("1.2.4"));
    }

    #[test]
    fn pruning_with_an_empty_keep_list_is_a_no_op() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_host_info("nodekey:1", "{}").unwrap();
        assert_eq!(db.prune_host_info(&[]).unwrap(), 0);
        assert_eq!(db.list_host_info().unwrap().len(), 1);
    }

    #[test]
    fn set_user_role_ignores_owner() {
        let db = Db::open_in_memory().unwrap();
        let owner = db.create_user("sub-1", None, None, None, Role::Member).unwrap();
        let other = db.create_user("sub-2", None, None, None, Role::Member).unwrap();

        db.set_user_role(&other.id, Role::Owner).unwrap();
        assert_eq!(db.get_user(&other.id).unwrap().unwrap().role, Role::Member);

        db.set_user_role(&owner.id, Role::Admin).unwrap();
        assert_eq!(db.get_user(&owner.id).unwrap().unwrap().role, Role::Owner);
    }

    #[test]
    fn api_key_sessions_store_only_a_hash() {
        let db = Db::open_in_memory().unwrap();
        let session = db
            .create_api_key_session("my-secret-key", "abcd...", chrono::Duration::milliseconds(60_000))
            .unwrap();
        assert_eq!(session.kind, SessionKind::ApiKey);
        assert_eq!(session.api_key_hash.as_deref().unwrap().len(), 64);
        assert_ne!(session.api_key_hash.as_deref().unwrap(), "my-secret-key");
    }
}
