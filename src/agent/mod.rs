//! Host information agent.
//!
//! The Headscale API does not expose per-node `HostInfo`. Two interchangeable
//! backends collect it. `system` reads the local Tailscale daemon (netmap
//! first, status as fallback). `external` drives the upstream `hp_agent`
//! binary over its line protocol (`"sync\n"` in, one JSON line out). The
//! service stores the results in `host_info`, keyed by Tailscale node key.
//! Views join on that key.

pub mod tailscale;

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::Mutex;

use crate::config::{AgentBackend, AgentConfig};
use crate::db::Db;

/// Snapshot of the agent's health, surfaced on the settings page.
#[derive(Debug, Clone, Serialize)]
pub struct AgentStatus {
    pub enabled: bool,
    /// Why the agent is disabled, when it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub backend: &'static str,
    pub synced_at: Option<DateTime<Utc>>,
    pub node_count: usize,
    pub error: Option<String>,
    /// Which Tailscale data source produced the last sync.
    pub source: Option<String>,
    /// Approval URL, when the agent's node waits for registration.
    pub auth_url: Option<String>,
}

impl AgentStatus {
    pub fn disabled(reason: impl Into<String>) -> Self {
        Self {
            enabled: false,
            reason: Some(reason.into()),
            backend: "none",
            synced_at: None,
            node_count: 0,
            error: None,
            source: None,
            auth_url: None,
        }
    }
}

/// The agent service. Cheap to clone.
#[derive(Clone)]
pub struct AgentService {
    inner: Arc<Inner>,
}

struct Inner {
    db: Db,
    /// [`AgentService::reconfigure`] swaps it when a setting changes, so the
    /// service tracks the current configuration without a restart.
    config: std::sync::RwLock<AgentConfig>,
    /// Used to approve the agent's own registration automatically.
    headscale: std::sync::RwLock<Option<crate::headscale::ApiClient>>,
    /// Set when the agent cannot run at all (no API key, old Headscale, …).
    disabled_reason: std::sync::RwLock<Option<String>>,
    state: Mutex<AgentState>,
    /// Long-running child for the external backend.
    child: Mutex<Option<AgentChild>>,
    /// Wakes the refresher so a configuration change takes effect at once
    /// instead of waiting out the current interval.
    wake: tokio::sync::Notify,
}

#[derive(Default)]
struct AgentState {
    synced_at: Option<DateTime<Utc>>,
    error: Option<String>,
    auth_url: Option<String>,
    node_count: usize,
    source: Option<&'static str>,
}

struct AgentChild {
    /// Held so the process is reaped (and killed on drop) with the service.
    #[allow(dead_code)]
    process: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl AgentService {
    /// Builds the service, or one that reports itself as disabled.
    ///
    /// `headscale` is the admin API client. When present, the agent approves its
    /// own pending registration instead of leaving the operator to click
    /// through an auth URL.
    pub fn new(
        db: Db,
        config: AgentConfig,
        disabled_reason: Option<String>,
        headscale: Option<crate::headscale::ApiClient>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                db,
                config: std::sync::RwLock::new(config),
                headscale: std::sync::RwLock::new(headscale),
                disabled_reason: std::sync::RwLock::new(disabled_reason),
                state: Mutex::new(AgentState::default()),
                child: Mutex::new(None),
                wake: tokio::sync::Notify::new(),
            }),
        }
    }

    /// Swaps in a fresh configuration so a saved setting takes effect without a
    /// restart: the next refresh tick uses the new backend and socket.
    pub fn reconfigure(
        &self,
        config: AgentConfig,
        disabled_reason: Option<String>,
        headscale: Option<crate::headscale::ApiClient>,
    ) {
        *self.inner.config.write().expect("config lock") = config;
        *self.inner.disabled_reason.write().expect("reason lock") = disabled_reason;
        *self.inner.headscale.write().expect("headscale lock") = headscale;
        // Sync immediately rather than waiting out the current interval.
        self.inner.wake.notify_one();
    }

    fn config(&self) -> AgentConfig {
        self.inner.config.read().expect("config lock").clone()
    }

    fn reason(&self) -> Option<String> {
        self.inner.disabled_reason.read().expect("reason lock").clone()
    }

    fn client(&self) -> Option<crate::headscale::ApiClient> {
        self.inner
            .headscale
            .read()
            .expect("headscale lock")
            .clone()
    }

    pub fn is_enabled(&self) -> bool {
        self.config().enabled && self.reason().is_none()
    }

    pub fn name(&self) -> &'static str {
        match self.config().backend {
            AgentBackend::System => "system",
            AgentBackend::External => "external",
        }
    }

    pub fn cache_ttl(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.config().cache_ttl)
    }

    pub async fn status(&self) -> AgentStatus {
        if let Some(reason) = self.reason() {
            return AgentStatus::disabled(reason);
        }
        if !self.config().enabled {
            return AgentStatus::disabled(
                "The Sailplane agent is not enabled in the configuration.",
            );
        }

        let state = self.inner.state.lock().await;
        AgentStatus {
            enabled: true,
            reason: None,
            backend: self.name(),
            synced_at: state.synced_at,
            node_count: state.node_count,
            error: state.error.clone(),
            source: state.source.map(str::to_string),
            auth_url: state.auth_url.clone(),
        }
    }

    /// Host info keyed by Tailscale node key.
    pub async fn host_info(&self) -> Result<HashMap<String, Value>> {
        let db = self.inner.db.clone();
        let rows = db
            .run(|conn| {
                let mut stmt = conn.prepare("SELECT host_id, payload FROM host_info")?;
                let rows = stmt
                    .query_map([], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?;

        Ok(rows
            .into_iter()
            .filter_map(|(key, payload)| {
                serde_json::from_str(&payload)
                    .ok()
                    .map(|value| (key, value))
            })
            .collect())
    }

    /// Collects host info once and stores it.
    pub async fn sync(&self, known_node_keys: &[String]) -> Result<usize> {
        if !self.is_enabled() {
            anyhow::bail!(
                "{}",
                self.reason().unwrap_or_else(|| "the agent is not enabled".into())
            );
        }

        let config = self.config();
        let collected = match config.backend {
            AgentBackend::System => tailscale::collect(config.socket.as_deref()).await,
            AgentBackend::External => self
                .collect_external()
                .await
                .map(|hosts| (hosts, tailscale::Source::Cli)),
        };

        let mut state = self.inner.state.lock().await;

        match collected {
            Ok((hosts, source)) => {
                let count = hosts.len();
                for (key, value) in &hosts {
                    let payload =
                        serde_json::to_string(value).context("failed to encode host info")?;
                    self.inner.db.upsert_host_info(key, &payload)?;
                }

                // Drop data for nodes that no longer exist.
                let _ = self.inner.db.prune_host_info(known_node_keys);

                state.synced_at = Some(Utc::now());
                state.node_count = count;
                state.error = None;
                state.auth_url = None;
                state.source = Some(source.as_str());
                Ok(count)
            }
            Err(err) => {
                let message = format!("{err:#}");
                state.error = Some(message.clone());
                Err(anyhow::anyhow!(message))
            }
        }
    }

    /// Drives the external `hp_agent` binary over its line protocol.
    async fn collect_external(&self) -> Result<HashMap<String, Value>> {
        let mut guard = self.inner.child.lock().await;

        if guard.is_none() {
            *guard = Some(self.spawn_child().await?);
        }

        let child = guard.as_mut().expect("just ensured");

        child
            .stdin
            .write_all(b"sync\n")
            .await
            .context("failed to write to the agent process")?;
        child
            .stdin
            .flush()
            .await
            .context("failed to flush the agent process stdin")?;

        let mut line = String::new();
        let read = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            child.stdout.read_line(&mut line),
        )
        .await
        .context("the agent process did not respond within 30s")??;

        if read == 0 {
            *guard = None;
            anyhow::bail!("the agent process exited unexpectedly");
        }

        let response: Value =
            serde_json::from_str(&line).context("the agent returned invalid JSON")?;

        if let Some(error) = response.get("error").and_then(Value::as_str) {
            // An approval URL means the agent's node is not registered yet.
            if let Some(url) = extract_auth_url(error) {
                let approved = self.try_auto_approve(&url).await;
                let mut state = self.inner.state.lock().await;
                state.auth_url = (!approved).then_some(url);
            }
            anyhow::bail!("{error}");
        }

        let hosts = response
            .get("hosts")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();

        Ok(hosts.into_iter().collect())
    }

    /// Approves the agent's own registration using the admin API key.
    ///
    /// Returns whether it succeeded. On failure, the auth URL is surfaced in the
    /// UI so an operator can approve it by hand.
    async fn try_auto_approve(&self, auth_url: &str) -> bool {
        let Some(client) = self.client() else {
            return false;
        };
        let Some(auth_id) = auth_id_from_url(auth_url) else {
            return false;
        };

        match client.approve_auth(&auth_id).await {
            Ok(()) => {
                tracing::info!("auto-approved the agent's registration");
                true
            }
            Err(err) => {
                tracing::warn!("could not auto-approve the agent registration: {err}");
                false
            }
        }
    }

    async fn spawn_child(&self) -> Result<AgentChild> {
        let config = self.config();
        let executable = &config.executable_path;
        // The external backend spawns the upstream `hp_agent` binary. These
        // names are that binary's contract. Do not rename them.
        let mut process = tokio::process::Command::new(executable)
            .env("HEADPLANE_AGENT_HOSTNAME", &config.host_name)
            .env("HEADPLANE_AGENT_WORK_DIR", &config.work_dir)
            .env(
                "HEADPLANE_AGENT_TS_NETNS",
                if config.tailscale_netns {
                    "true"
                } else {
                    "false"
                },
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| {
                format!(
                    "failed to start the agent binary at {}; set \
                     `integration.agent.backend: system` to use the host Tailscale client instead",
                    executable.display()
                )
            })?;

        let stdin = process.stdin.take().context("agent stdin unavailable")?;
        let stdout = process.stdout.take().context("agent stdout unavailable")?;

        Ok(AgentChild {
            process,
            stdin,
            stdout: BufReader::new(stdout),
        })
    }

    /// Spawns a background refresher that syncs every `cache_ttl`.
    ///
    /// Node keys come from the live store so host info for deleted nodes is
    /// pruned automatically. The loop runs even while the agent is off, so
    /// enabling it from the web starts collecting on the next tick without a
    /// restart.
    pub fn spawn_refresher(self: &Arc<Self>, live: Arc<crate::headscale::LiveStore>) {
        let this = Arc::clone(self);

        tokio::spawn(async move {
            // Give the live store a moment to load before the first sync.
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;

            loop {
                // Read the interval each pass so a changed cache_ttl applies.
                let interval = this.cache_ttl().max(std::time::Duration::from_secs(30));

                if this.is_enabled() {
                    let keys: Vec<String> = live
                        .nodes()
                        .await
                        .data
                        .iter()
                        .map(|node| node.node_key.clone())
                        .collect();

                    if let Err(err) = this.sync(&keys).await {
                        tracing::debug!("agent sync failed: {err:#}");
                    }
                }

                // Sleep until the interval elapses, or a saved setting wakes us
                // early, so a change is reflected without an immediate restart.
                tokio::select! {
                    _ = tokio::time::sleep(interval) => {}
                    _ = this.inner.wake.notified() => {}
                }
            }
        });
    }
}

/// Explains why the agent cannot run, or `None` when it can.
///
/// Recomputed whenever a setting is saved, so the reason tracks the current
/// configuration instead of being frozen at startup.
pub fn disabled_reason(
    config: &crate::config::Config,
    headscale: &crate::headscale::Headscale,
) -> Option<String> {
    let agent_config = config.integration.as_ref()?.agent.as_ref()?;
    if !agent_config.enabled {
        return Some("The Sailplane agent is disabled in the configuration.".into());
    }
    if config.headscale.api_key.is_none() {
        return Some(
            "The agent cannot register itself or read node data without `headscale.api_key`."
                .into(),
        );
    }
    if !headscale.capabilities().agent_supported() {
        return Some(
            "The agent can use tag-only pre-authentication keys only on Headscale 0.28.0 or newer."
                .into(),
        );
    }
    if agent_config.backend == AgentBackend::System {
        let socket = agent_config
            .socket
            .clone()
            .unwrap_or_else(|| std::path::PathBuf::from(tailscale::DEFAULT_SOCKET));
        if !socket.exists() {
            return Some(format!(
                "No Tailscale daemon socket at {}. Mount the socket into the container, or set \
                 `integration.agent.socket` to its path.",
                socket.display()
            ));
        }
    }
    None
}

/// The agent reports an auth URL when its node waits for approval. The last
/// path segment is the auth id for `POST /api/v1/auth/approve`.
pub fn extract_auth_url(message: &str) -> Option<String> {
    message
        .split_whitespace()
        .find(|token| token.starts_with("http://") || token.starts_with("https://"))
        .map(|url| url.trim_end_matches(['.', ',']).to_string())
}

/// The auth id embedded in a registration URL.
pub fn auth_id_from_url(url: &str) -> Option<String> {
    let without_query = url.split('?').next().unwrap_or(url);
    without_query
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent_config(enabled: bool) -> AgentConfig {
        AgentConfig {
            enabled,
            host_name: "sailplane-agent".into(),
            cache_ttl: 180_000,
            backend: AgentBackend::System,
            socket: None,
            executable_path: "/usr/libexec/sailplane/agent".into(),
            work_dir: "/var/lib/sailplane/agent".into(),
            tailscale_netns: true,
        }
    }

    #[test]
    fn extracts_auth_url_from_agent_output() {
        let message = "please visit https://headscale.example.com/register/nodekey:abc to approve";
        assert_eq!(
            extract_auth_url(message).as_deref(),
            Some("https://headscale.example.com/register/nodekey:abc")
        );
        assert_eq!(extract_auth_url("no url here"), None);
    }

    #[test]
    fn auth_id_is_the_last_path_segment() {
        assert_eq!(
            auth_id_from_url("https://headscale.example.com/register/abc123?x=1").as_deref(),
            Some("abc123")
        );
        assert_eq!(
            auth_id_from_url("https://example.com/a/b/").as_deref(),
            Some("b")
        );
    }

    #[test]
    fn disabled_agent_reports_a_reason() {
        let db = Db::open_in_memory().unwrap();
        let service = AgentService::new(db, agent_config(true), Some("no API key".into()), None);
        assert!(!service.is_enabled());

        let status = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(service.status());
        assert!(!status.enabled);
        assert_eq!(status.reason.as_deref(), Some("no API key"));
    }

    #[test]
    fn reconfiguring_takes_effect_without_a_restart() {
        let db = Db::open_in_memory().unwrap();
        let service = AgentService::new(db, agent_config(false), None, None);
        assert!(!service.is_enabled());

        // Enabling from the web swaps the configuration in place.
        service.reconfigure(agent_config(true), None, None);
        assert!(service.is_enabled());
        assert_eq!(service.name(), "system");

        // A configuration that cannot run still reports disabled.
        service.reconfigure(agent_config(true), Some("no socket".into()), None);
        assert!(!service.is_enabled());
    }
}
