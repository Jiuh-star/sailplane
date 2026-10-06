//! Host info collected from a Tailscale daemon.
//!
//! The Headscale API does not expose per-node `HostInfo`, so the data comes from
//! a Tailscale node on the tailnet via LocalAPI: netmap, then status, then the
//! `tailscale` CLI. Netmap needs root or `--operator`; status cannot report peer
//! versions. Results are keyed by node key, as Headscale's `nodeKey`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::{Map, Value};

use crate::unix_socket;

/// Where a default tailscaled listens on Linux.
pub const DEFAULT_SOCKET: &str = "/var/run/tailscale/tailscaled.sock";

/// The `Host` sentinel tailscaled requires on LocalAPI requests.
const LOCALAPI_HOST: &str = "local-tailscaled.sock";

/// How long the CLI fallback may take.
const CLI_TIMEOUT: Duration = Duration::from_secs(15);

/// Where host info came from, surfaced in the agent status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Netmap,
    Status,
    Cli,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Netmap => "netmap",
            Self::Status => "status",
            Self::Cli => "tailscale-cli",
        }
    }
}

/// Collects host info, trying each source in turn.
///
/// Returns the data and the source that produced it, so the UI can explain
/// which fields are available.
pub async fn collect(socket: Option<&Path>) -> Result<(HashMap<String, Value>, Source)> {
    let socket = socket.map(Path::to_path_buf);
    let mut failures: Vec<String> = Vec::new();

    if let Some(socket) = socket.as_deref() {
        match from_netmap(socket).await {
            Ok(hosts) if !hosts.is_empty() => return Ok((hosts, Source::Netmap)),
            Ok(_) => failures.push("the netmap contained no peers".into()),
            Err(err) => failures.push(format!("netmap: {err:#}")),
        }

        match from_status(socket).await {
            Ok(hosts) if !hosts.is_empty() => return Ok((hosts, Source::Status)),
            Ok(_) => failures.push("the daemon reported no peers".into()),
            Err(err) => failures.push(format!("status: {err:#}")),
        }
    }

    match from_cli().await {
        Ok(hosts) if !hosts.is_empty() => {
            // A CLI fallback cannot report peer versions, so the version
            // column stays empty.
            tracing::warn!(
                "the Tailscale daemon socket was not usable ({}); falling back to the CLI, \
                 which cannot report peer versions",
                failures.join("; ")
            );
            return Ok((hosts, Source::Cli));
        }
        Ok(_) => failures.push("the CLI reported no peers".into()),
        Err(err) => failures.push(format!("cli: {err:#}")),
    }

    anyhow::bail!(
        "no Tailscale data source responded ({})",
        failures.join("; ")
    )
}

/// Reports which source is usable without doing a full collection.
pub async fn probe(socket: Option<&Path>) -> Option<Source> {
    let socket = socket?;
    if let Ok(response) = unix_socket::request_from(
        socket,
        "POST",
        "/localapi/v0/debug?action=current-netmap",
        None,
        Some(LOCALAPI_HOST),
    )
    .await
        && response.is_success()
    {
        return Some(Source::Netmap);
    }

    // Any other status (403 for a non-root caller, 404 on an older daemon)
    // means debug actions are unavailable; the status endpoint still works.
    if let Ok(response) = unix_socket::request_from(
        socket,
        "GET",
        "/localapi/v0/status",
        None,
        Some(LOCALAPI_HOST),
    )
    .await
        && response.is_success()
    {
        return Some(Source::Status);
    }

    None
}

async fn from_netmap(socket: &Path) -> Result<HashMap<String, Value>> {
    let response = unix_socket::request_from(
        socket,
        "POST",
        "/localapi/v0/debug?action=current-netmap",
        Some(b"{}".to_vec()),
        Some(LOCALAPI_HOST),
    )
    .await?;

    if !response.is_success() {
        anyhow::bail!(
            "the daemon answered {}: {}",
            response.status,
            response.text()
        );
    }

    let netmap: Value = response.json()?;
    Ok(map_netmap(&netmap))
}

async fn from_status(socket: &Path) -> Result<HashMap<String, Value>> {
    let response = unix_socket::request_from(
        socket,
        "GET",
        "/localapi/v0/status",
        None,
        Some(LOCALAPI_HOST),
    )
    .await?;

    if !response.is_success() {
        anyhow::bail!(
            "the daemon answered {}: {}",
            response.status,
            response.text()
        );
    }

    let status: Value = response.json()?;
    Ok(map_status(&status))
}

async fn from_cli() -> Result<HashMap<String, Value>> {
    let output = tokio::time::timeout(
        CLI_TIMEOUT,
        tokio::process::Command::new("tailscale")
            .args(["status", "--json"])
            .output(),
    )
    .await
    .context("`tailscale status --json` timed out")?
    .context("failed to run `tailscale status`; is the Tailscale client installed?")?;

    if !output.status.success() {
        anyhow::bail!(
            "`tailscale status --json` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let status: Value =
        serde_json::from_slice(&output.stdout).context("failed to parse the status output")?;
    Ok(map_status(&status))
}

/// Projects a netmap document onto the host-info shape the UI consumes.
fn map_netmap(netmap: &Value) -> HashMap<String, Value> {
    let mut hosts = HashMap::new();

    if let Some(peers) = netmap.get("Peers").and_then(Value::as_array) {
        for peer in peers {
            if let Some(key) = peer.get("Key").and_then(Value::as_str) {
                hosts.insert(key.to_string(), host_info_from_netmap_node(peer));
            }
        }
    }

    if let Some(self_node) = netmap.get("SelfNode")
        && let Some(key) = self_node.get("Key").and_then(Value::as_str)
    {
        let mut info = host_info_from_netmap_node(self_node);
        if let Some(map) = info.as_object_mut() {
            map.insert("SailplaneAgent".into(), Value::Bool(true));
        }
        hosts.insert(key.to_string(), info);
    }

    hosts
}

fn host_info_from_netmap_node(node: &Value) -> Value {
    let mut info = Map::new();

    let hostinfo = node.get("Hostinfo").and_then(Value::as_object);

    // Copy the Hostinfo fields the UI renders, verbatim.
    if let Some(hostinfo) = hostinfo {
        for key in [
            "IPNVersion",
            "OS",
            "OSVersion",
            "Distro",
            "DistroVersion",
            "Hostname",
            "DeviceModel",
            "GoArch",
            "GoVersion",
            "Userspace",
            "Container",
            "Services",
            "NetInfo",
            "sshHostKeys",
            "RoutableIPs",
            "RequestTags",
        ] {
            if let Some(value) = hostinfo.get(key)
                && !value.is_null()
            {
                info.insert(key.into(), value.clone());
            }
        }
    }

    if let Some(endpoints) = node.get("Endpoints").and_then(Value::as_array) {
        info.insert("Endpoints".into(), Value::Array(endpoints.clone()));
    }
    if let Some(addresses) = node.get("Addresses").and_then(Value::as_array) {
        info.insert("TailscaleIPs".into(), Value::Array(addresses.clone()));
    }
    if let Some(derp) = node.get("HomeDERP")
        && !derp.is_null()
    {
        info.insert("HomeDERP".into(), derp.clone());
    }
    if let Some(online) = node.get("Online") {
        info.insert("Online".into(), online.clone());
    }
    if let Some(name) = node.get("ComputedName").and_then(Value::as_str) {
        info.insert("ComputedName".into(), Value::String(name.to_string()));
    }

    Value::Object(info)
}

/// Projects a `tailscale status --json` document onto the same shape.
fn map_status(status: &Value) -> HashMap<String, Value> {
    let mut hosts = HashMap::new();

    if let Some(peers) = status.get("Peer").and_then(Value::as_object) {
        for (node_key, peer) in peers {
            hosts.insert(node_key.clone(), host_info_from_status_peer(peer));
        }
    }

    if let Some(self_node) = status.get("Self") {
        let key = self_node
            .get("PublicKey")
            .and_then(Value::as_str)
            .unwrap_or("self");
        let mut info = host_info_from_status_peer(self_node);
        if let Some(map) = info.as_object_mut() {
            if let Some(version) = status.get("Version").and_then(Value::as_str) {
                map.insert("IPNVersion".into(), Value::String(version.into()));
            }
            map.insert("SailplaneAgent".into(), Value::Bool(true));
        }
        hosts.insert(key.to_string(), info);
    }

    hosts
}

fn host_info_from_status_peer(peer: &Value) -> Value {
    let mut info = Map::new();

    if let Some(hostinfo) = peer.get("Hostinfo").and_then(Value::as_object) {
        // A locally-installed client reports its own host info, and newer
        // versions include peer host info too.
        for key in [
            "IPNVersion",
            "OS",
            "OSVersion",
            "Distro",
            "DeviceModel",
            "NetInfo",
        ] {
            if let Some(value) = hostinfo.get(key)
                && !value.is_null()
            {
                info.insert(key.into(), value.clone());
            }
        }
    }

    for (field, target) in [("HostName", "Hostname"), ("OS", "OS")] {
        if let Some(value) = peer.get(field).and_then(Value::as_str)
            && !value.is_empty()
            && !info.contains_key(target)
        {
            info.insert(target.into(), Value::String(value.into()));
        }
    }

    if let Some(addresses) = peer.get("TailscaleIPs").and_then(Value::as_array) {
        info.insert("TailscaleIPs".into(), Value::Array(addresses.clone()));
    }

    // The direct address when connected, otherwise the relay.
    if let Some(cur_addr) = peer.get("CurAddr").and_then(Value::as_str)
        && !cur_addr.is_empty()
    {
        info.insert(
            "Endpoints".into(),
            Value::Array(vec![Value::String(cur_addr.into())]),
        );
    }
    if let Some(relay) = peer.get("Relay").and_then(Value::as_str)
        && !relay.is_empty()
    {
        info.insert("HomeDERP".into(), Value::String(relay.into()));
    }

    if let Some(tags) = peer.get("Tags").and_then(Value::as_array) {
        info.insert("RequestTags".into(), Value::Array(tags.clone()));
    }
    if let Some(routes) = peer.get("PrimaryRoutes").and_then(Value::as_array) {
        info.insert("RoutableIPs".into(), Value::Array(routes.clone()));
    }
    if let Some(ssh_keys) = peer.get("sshHostKeys").and_then(Value::as_array) {
        info.insert("sshHostKeys".into(), Value::Array(ssh_keys.clone()));
    }

    if let Some(online) = peer.get("Online") {
        info.insert("Online".into(), online.clone());
    }

    // A peer that advertises the SSH capability but predates `sshHostKeys`
    // still counts as reachable over Tailscale SSH.
    if !info.contains_key("sshHostKeys")
        && let Some(capabilities) = peer.get("Capabilities").and_then(Value::as_array)
    {
        let has_ssh = capabilities
            .iter()
            .filter_map(Value::as_str)
            .any(|capability| capability.contains("ssh"));
        if has_ssh {
            info.insert("sshHostKeys".into(), Value::Array(vec![]));
        }
    }

    Value::Object(info)
}

/// The default socket path, if the daemon socket exists.
pub fn default_socket() -> Option<PathBuf> {
    let path = PathBuf::from(DEFAULT_SOCKET);
    path.exists().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NETMAP: &str = r#"{
        "Peers": [
            {
                "Key": "nodekey:aaa",
                "Name": "web-01.tailnet.ts.net.",
                "ComputedName": "web-01",
                "Endpoints": ["203.0.113.5:41641"],
                "HomeDERP": 7,
                "Online": true,
                "Hostinfo": {
                    "IPNVersion": "1.82.0",
                    "OS": "linux",
                    "OSVersion": "6.9.7",
                    "Hostname": "web-01",
                    "NetInfo": { "WorkingUDP": true, "HairPinning": false },
                    "sshHostKeys": ["ssh-ed25519 AAAA"]
                }
            }
        ],
        "SelfNode": {
            "Key": "nodekey:self",
            "ComputedName": "sailplane-agent",
            "Hostinfo": { "IPNVersion": "1.84.0", "OS": "linux" }
        }
    }"#;

    #[test]
    fn maps_netmap_peer_hostinfo() {
        let netmap: Value = serde_json::from_str(NETMAP).unwrap();
        let hosts = map_netmap(&netmap);

        assert_eq!(hosts.len(), 2);

        let peer = &hosts["nodekey:aaa"];
        assert_eq!(peer["IPNVersion"], "1.82.0");
        assert_eq!(peer["OS"], "linux");
        assert_eq!(peer["Hostname"], "web-01");
        assert_eq!(peer["Endpoints"][0], "203.0.113.5:41641");
        assert_eq!(peer["HomeDERP"], 7);
        assert_eq!(peer["Online"], true);
        assert_eq!(peer["NetInfo"]["WorkingUDP"], true);
        assert_eq!(peer["sshHostKeys"][0], "ssh-ed25519 AAAA");

        let me = &hosts["nodekey:self"];
        assert_eq!(me["IPNVersion"], "1.84.0");
        assert_eq!(me["SailplaneAgent"], true);
    }

    #[test]
    fn maps_status_peers_without_hostinfo() {
        let status: Value = serde_json::json!({
            "Version": "1.102.5",
            "Self": { "PublicKey": "nodekey:self", "HostName": "firebat", "OS": "linux" },
            "Peer": {
                "nodekey:bbb": {
                    "HostName": "phone",
                    "OS": "iOS",
                    "TailscaleIPs": ["100.64.0.9"],
                    "CurAddr": "",
                    "Relay": "sin",
                    "Online": false
                }
            }
        });

        let hosts = map_status(&status);
        let peer = &hosts["nodekey:bbb"];
        assert_eq!(peer["OS"], "iOS");
        assert_eq!(peer["Hostname"], "phone");
        assert_eq!(peer["HomeDERP"], "sin");
        assert_eq!(peer["Online"], false);
        // The status projection has no version for peers.
        assert!(peer.get("IPNVersion").is_none());

        // …but the local node does, from the top-level `Version`.
        assert_eq!(hosts["nodekey:self"]["IPNVersion"], "1.102.5");
    }

    #[test]
    fn ssh_capability_implies_host_keys() {
        let status: Value = serde_json::json!({
            "Peer": {
                "nodekey:ccc": {
                    "HostName": "server",
                    "Capabilities": ["https://tailscale.com/cap/ssh"]
                }
            }
        });
        let hosts = map_status(&status);
        assert!(hosts["nodekey:ccc"]["sshHostKeys"].is_array());
    }

    #[test]
    fn empty_documents_produce_no_hosts() {
        assert!(map_netmap(&serde_json::json!({})).is_empty());
        assert!(map_status(&serde_json::json!({})).is_empty());
    }
}
