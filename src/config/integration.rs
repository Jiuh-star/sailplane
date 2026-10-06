//! `integration.*` configuration: how Sailplane reloads Headscale after
//! touching its config file, plus the optional host-info agent.

use std::path::PathBuf;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

/// Browser SSH settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[derive(Default)]
pub struct SshConfig {
    #[serde(default)]
    pub enabled: bool,

    /// TCP port on the target. Defaults to 22.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,

    /// Default login name. The UI lets the user pick another one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,

    /// SOCKS5 proxy used to reach the tailnet, usually the sidecar's
    /// `TS_SOCKS5_SERVER`. Connections are direct when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy: Option<String>,

    /// Private key for targets that use ordinary sshd authentication.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key_path: Option<PathBuf>,

    /// Password authentication. Prefer a key; this exists for appliances.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IntegrationConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docker: Option<DockerConfig>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kubernetes: Option<KubernetesConfig>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proc: Option<ProcConfig>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentConfig>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh: Option<SshConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DockerConfig {
    pub enabled: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container_name: Option<String>,

    #[serde(default = "default_container_label")]
    pub container_label: String,

    #[serde(default = "default_docker_socket")]
    pub socket: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct KubernetesConfig {
    pub enabled: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pod_name: Option<String>,

    #[serde(default = "default_true")]
    pub validate_manifest: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProcConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentConfig {
    pub enabled: bool,

    #[serde(default = "default_agent_host_name")]
    pub host_name: String,

    /// How long a host-info snapshot stays fresh, in milliseconds.
    #[serde(default = "default_cache_ttl")]
    pub cache_ttl: u64,

    #[serde(default)]
    pub backend: AgentBackend,

    /// Tailscaled socket to query when `backend = system`. Defaults to
    /// `/var/run/tailscale/tailscaled.sock`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub socket: Option<PathBuf>,

    /// Path to an `hp_agent` binary when `backend = external`.
    #[serde(default = "default_executable_path")]
    pub executable_path: PathBuf,

    #[serde(default = "default_work_dir")]
    pub work_dir: PathBuf,

    #[serde(default = "default_true")]
    pub tailscale_netns: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AgentBackend {
    /// Uses the host's `tailscale status --json` output.
    #[default]
    System,
    /// Spawns a prebuilt `hp_agent` process and speaks its line protocol.
    External,
}

fn default_true() -> bool {
    true
}

fn default_container_label() -> String {
    "sailplane.target=headscale".into()
}

fn default_docker_socket() -> String {
    "unix:///var/run/docker.sock".into()
}

fn default_agent_host_name() -> String {
    "sailplane-agent".into()
}

fn default_cache_ttl() -> u64 {
    180_000
}

fn default_executable_path() -> PathBuf {
    PathBuf::from("/usr/libexec/sailplane/agent")
}

fn default_work_dir() -> PathBuf {
    PathBuf::from("/var/lib/sailplane/agent")
}

impl IntegrationConfig {
    pub(super) fn validate(&mut self) -> Result<()> {
        let enabled: Vec<&str> = [
            self.docker.as_ref().filter(|c| c.enabled).map(|_| "docker"),
            self.kubernetes
                .as_ref()
                .filter(|c| c.enabled)
                .map(|_| "kubernetes"),
            self.proc.as_ref().filter(|c| c.enabled).map(|_| "proc"),
        ]
        .into_iter()
        .flatten()
        .collect();

        if enabled.len() > 1 {
            bail!(
                "only one of integration.docker, integration.kubernetes or integration.proc \
                 may be enabled at a time (found: {})",
                enabled.join(", ")
            );
        }

        if let Some(docker) = self.docker.as_ref()
            && !docker.socket.starts_with("unix://")
            && !docker.socket.starts_with("tcp://")
        {
            bail!("integration.docker.socket must start with unix:// or tcp://");
        }

        Ok(())
    }
}
