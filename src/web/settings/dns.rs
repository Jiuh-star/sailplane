//! DNS settings backed by the Headscale configuration file.

use axum::Json;
use axum::extract::State;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::auth::Capability;
use crate::hsconfig::DnsRecord;
use crate::hsconfig::yaml_edit::parse_path;

use super::super::error::{ApiError, ApiResult};
use super::super::state::{Auth, PrincipalExt, SharedState};
use super::reload_after_change;

/// Reads `dns.override_local_dns`, defaulting to Headscale's own default.
fn current_override(state: &SharedState) -> Result<bool, ApiError> {
    let document = state
        .hsconfig
        .document()
        .map_err(|err| ApiError::internal(format!("{err:#}")))?;
    Ok(document
        .get_bool(&parse_path("dns.override_local_dns"))
        .unwrap_or(true))
}

/// Makes sure that the caller can edit network settings and that the config is
/// writable.
fn require_writable(
    state: &SharedState,
    principal: &crate::auth::Principal,
) -> Result<(), ApiError> {
    principal.require(&[Capability::WriteNetwork])?;
    if !state.hsconfig.writable() {
        return Err(ApiError::forbidden(
            "The Headscale configuration file is not writable, so DNS settings are read-only",
        ));
    }
    Ok(())
}

/// Returns the DNS configuration. `GET /api/dns`
pub async fn get_config(
    State(state): State<SharedState>,
    Auth(principal): Auth,
) -> ApiResult<Json<Value>> {
    if !principal.has(Capability::ReadNetwork) {
        return Err(ApiError::forbidden(
            "Your account does not have access to DNS settings",
        ));
    }

    let dns = state
        .hsconfig
        .dns_config()
        .map_err(|err| ApiError::internal(format!("{err:#}")))?;

    Ok(Json(json!({
        "dns": dns,
        "access": {
            "read": true,
            "write": principal.has(Capability::WriteNetwork),
            "writable": state.hsconfig.writable(),
            "available": state.hsconfig.readable(),
        },
        "integration": state.integration.name(),
    })))
}

#[derive(Deserialize)]
pub struct TailnetRequest {
    new_name: String,
}

/// Renames the MagicDNS base domain. `POST /api/dns/tailnet`
pub async fn rename_tailnet(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Json(request): Json<TailnetRequest>,
) -> ApiResult<Json<Value>> {
    require_writable(&state, &principal)?;

    let name = request.new_name.trim();
    if !crate::util::is_valid_dns_name(name) {
        return Err(ApiError::bad_request("Enter a valid domain name"));
    }

    state
        .hsconfig
        .patch(&[(
            parse_path("dns.base_domain"),
            Some(Value::String(name.to_string())),
        )])
        .await
        .map_err(|err| ApiError::internal(format!("{err:#}")))?;

    let warning = reload_after_change(&state).await;
    Ok(Json(json!({ "ok": true, "warning": warning })))
}

#[derive(Deserialize)]
pub struct MagicRequest {
    enabled: bool,
}

/// Toggles MagicDNS. `POST /api/dns/magic`
pub async fn toggle_magic(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Json(request): Json<MagicRequest>,
) -> ApiResult<Json<Value>> {
    require_writable(&state, &principal)?;

    state
        .hsconfig
        .patch(&[(
            parse_path("dns.magic_dns"),
            Some(Value::Bool(request.enabled)),
        )])
        .await
        .map_err(|err| ApiError::internal(format!("{err:#}")))?;

    let warning = reload_after_change(&state).await;
    Ok(Json(json!({ "ok": true, "warning": warning })))
}

#[derive(Deserialize)]
pub struct OverrideRequest {
    enabled: bool,
}

/// Toggles `dns.override_local_dns`. `POST /api/dns/override`
pub async fn set_override(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Json(request): Json<OverrideRequest>,
) -> ApiResult<Json<Value>> {
    require_writable(&state, &principal)?;

    // The reverse invariant: Headscale refuses to start when the override is
    // enabled with no global nameservers.
    if request.enabled {
        let servers = state
            .hsconfig
            .document()
            .map_err(|err| ApiError::internal(format!("{err:#}")))?
            .get_string_list(&parse_path("dns.nameservers.global"));
        if servers.is_empty() {
            return Err(ApiError::bad_request(
                "Add at least one global nameserver before enabling “Override local DNS”. \
                 Headscale refuses to start with an empty list.",
            ));
        }
    }

    state
        .hsconfig
        .patch(&[(
            parse_path("dns.override_local_dns"),
            Some(Value::Bool(request.enabled)),
        )])
        .await
        .map_err(|err| ApiError::internal(format!("{err:#}")))?;

    let warning = reload_after_change(&state).await;
    Ok(Json(json!({ "ok": true, "warning": warning })))
}

#[derive(Deserialize)]
pub struct NameserverRequest {
    ns: String,
    /// Empty or `global` targets the global list. Anything else is split DNS.
    #[serde(default)]
    split_name: Option<String>,
}

/// Rejects a split-DNS name that is not a domain: the name becomes a key in
/// the Headscale config, so it must not carry YAML structure.
fn validate_split_name(split_name: Option<&str>) -> ApiResult<()> {
    let Some(name) = split_name.map(str::trim).filter(|name| !name.is_empty()) else {
        return Ok(());
    };
    if name == "global" || crate::util::is_valid_dns_name(name) {
        return Ok(());
    }
    Err(ApiError::bad_request(
        "Enter a valid domain name for the split DNS entry",
    ))
}

fn split_path(split_name: Option<&str>, suffix: &str) -> String {
    match split_name.map(str::trim).filter(|name| !name.is_empty()) {
        Some(name) if name != "global" => {
            format!("dns.nameservers.split.\"{name}\"{}", suffix)
        }
        _ => format!("dns.nameservers.global{suffix}"),
    }
}

/// Adds a nameserver. `POST /api/dns/nameservers`
pub async fn add_nameserver(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Json(request): Json<NameserverRequest>,
) -> ApiResult<Json<Value>> {
    require_writable(&state, &principal)?;

    let ns = request.ns.trim();
    if ns.parse::<std::net::IpAddr>().is_err() {
        return Err(ApiError::bad_request("Enter a valid IP address"));
    }
    validate_split_name(request.split_name.as_deref())?;

    let path = split_path(request.split_name.as_deref(), "");
    let mut servers = state
        .hsconfig
        .document()
        .map_err(|err| ApiError::internal(format!("{err:#}")))?
        .get_string_list(&parse_path(&path));

    if servers.iter().any(|existing| existing == ns) {
        return Err(ApiError::bad_request(
            "That nameserver is already configured",
        ));
    }
    servers.push(ns.to_string());

    state
        .hsconfig
        .patch(&[(parse_path(&path), Some(json!(servers)))])
        .await
        .map_err(|err| ApiError::internal(format!("{err:#}")))?;

    let warning = reload_after_change(&state).await;
    Ok(Json(json!({ "ok": true, "warning": warning })))
}

/// Removes a nameserver. `POST /api/dns/nameservers/remove`
pub async fn remove_nameserver(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Json(request): Json<NameserverRequest>,
) -> ApiResult<Json<Value>> {
    require_writable(&state, &principal)?;

    let ns = request.ns.trim();
    let is_global = request
        .split_name
        .as_deref()
        .map(|name| name.trim().is_empty() || name == "global")
        .unwrap_or(true);
    validate_split_name(request.split_name.as_deref())?;
    let path = split_path(request.split_name.as_deref(), "");

    let mut servers = state
        .hsconfig
        .document()
        .map_err(|err| ApiError::internal(format!("{err:#}")))?
        .get_string_list(&parse_path(&path));

    servers.retain(|existing| existing != ns);

    // Headscale refuses to start when `override_local_dns` is set and the
    // global list is empty. Refuse rather than write a config that fails on
    // restart.
    if is_global && servers.is_empty() && current_override(&state)? {
        return Err(ApiError::bad_request(
            "This is the last global nameserver and `override_local_dns` is enabled. Headscale \
             refuses to start with an empty list. Add another nameserver first, or turn off \
             “Override local DNS”.",
        ));
    }

    // Removing the last split entry deletes the key rather than leaving an
    // empty list behind.
    let value = if servers.is_empty() && !is_global {
        None
    } else {
        Some(json!(servers))
    };

    state
        .hsconfig
        .patch(&[(parse_path(&path), value)])
        .await
        .map_err(|err| ApiError::internal(format!("{err:#}")))?;

    let warning = reload_after_change(&state).await;
    Ok(Json(json!({ "ok": true, "warning": warning })))
}

#[derive(Deserialize)]
pub struct SearchDomainRequest {
    domain: Option<String>,
}

/// Adds a search domain. `POST /api/dns/search-domains`
pub async fn add_search_domain(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Json(request): Json<SearchDomainRequest>,
) -> ApiResult<Json<Value>> {
    require_writable(&state, &principal)?;

    let Some(domain) = request
        .domain
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
    else {
        return Err(ApiError::bad_request("Enter a search domain"));
    };

    if !crate::util::is_valid_dns_name(domain) {
        return Err(ApiError::bad_request("Enter a valid domain name"));
    }

    let mut domains = state
        .hsconfig
        .document()
        .map_err(|err| ApiError::internal(format!("{err:#}")))?
        .get_string_list(&parse_path("dns.search_domains"));

    if domains.iter().any(|existing| existing == domain) {
        return Err(ApiError::bad_request(
            "That search domain is already configured",
        ));
    }
    domains.push(domain.to_string());

    state
        .hsconfig
        .patch(&[(parse_path("dns.search_domains"), Some(json!(domains)))])
        .await
        .map_err(|err| ApiError::internal(format!("{err:#}")))?;

    let warning = reload_after_change(&state).await;
    Ok(Json(json!({ "ok": true, "warning": warning })))
}

/// Removes a search domain. `POST /api/dns/search-domains/remove`
pub async fn remove_search_domain(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Json(request): Json<SearchDomainRequest>,
) -> ApiResult<Json<Value>> {
    require_writable(&state, &principal)?;

    let Some(domain) = request.domain.as_deref().map(str::trim) else {
        return Err(ApiError::bad_request("A domain is required"));
    };

    let mut domains = state
        .hsconfig
        .document()
        .map_err(|err| ApiError::internal(format!("{err:#}")))?
        .get_string_list(&parse_path("dns.search_domains"));

    domains.retain(|existing| existing != domain);

    state
        .hsconfig
        .patch(&[(parse_path("dns.search_domains"), Some(json!(domains)))])
        .await
        .map_err(|err| ApiError::internal(format!("{err:#}")))?;

    let warning = reload_after_change(&state).await;
    Ok(Json(json!({ "ok": true, "warning": warning })))
}

#[derive(Deserialize)]
pub struct RecordRequest {
    record_name: Option<String>,
    record_type: Option<String>,
    record_value: Option<String>,
    name: Option<String>,
    #[serde(rename = "type")]
    type_alias: Option<String>,
    value: Option<String>,
}

impl RecordRequest {
    fn parts(&self) -> (String, String, String) {
        (
            self.record_name
                .clone()
                .or_else(|| self.name.clone())
                .unwrap_or_default()
                .trim()
                .to_string(),
            self.record_type
                .clone()
                .or_else(|| self.type_alias.clone())
                .unwrap_or_default()
                .trim()
                .to_uppercase(),
            self.record_value
                .clone()
                .or_else(|| self.value.clone())
                .unwrap_or_default()
                .trim()
                .to_string(),
        )
    }
}

/// Adds a DNS record. `POST /api/dns/records`
pub async fn add_record(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Json(request): Json<RecordRequest>,
) -> ApiResult<Json<Value>> {
    require_writable(&state, &principal)?;

    let (name, record_type, value) = request.parts();
    if name.is_empty() || record_type.is_empty() || value.is_empty() {
        return Err(ApiError::bad_request(
            "Name, type and value are all required",
        ));
    }
    if !matches!(record_type.as_str(), "A" | "AAAA" | "CNAME") {
        return Err(ApiError::bad_request(
            "Record type must be A, AAAA or CNAME",
        ));
    }
    if !crate::util::is_valid_dns_name(&name) {
        return Err(ApiError::bad_request("Enter a valid record name"));
    }
    if record_type == "CNAME" {
        if !crate::util::is_valid_dns_name(&value) {
            return Err(ApiError::bad_request(
                "A CNAME target must be a domain name",
            ));
        }
    } else if value.parse::<std::net::IpAddr>().is_err() {
        return Err(ApiError::bad_request("Enter a valid IP address"));
    }

    state
        .hsconfig
        .add_dns_record(DnsRecord {
            name,
            record_type,
            value,
        })
        .await
        .map_err(|err| ApiError::bad_request(format!("{err:#}")))?;

    let warning = reload_after_change(&state).await;
    Ok(Json(json!({ "ok": true, "warning": warning })))
}

/// Removes a DNS record. `POST /api/dns/records/remove`
pub async fn remove_record(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Json(request): Json<RecordRequest>,
) -> ApiResult<Json<Value>> {
    require_writable(&state, &principal)?;

    let (name, record_type, value) = request.parts();
    if name.is_empty() || record_type.is_empty() {
        return Err(ApiError::bad_request("Name and type are required"));
    }

    state
        .hsconfig
        .remove_dns_record(&DnsRecord {
            name,
            record_type,
            value,
        })
        .await
        .map_err(|err| ApiError::internal(format!("{err:#}")))?;

    let warning = reload_after_change(&state).await;
    Ok(Json(json!({ "ok": true, "warning": warning })))
}
