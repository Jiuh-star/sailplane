//! Reading the relay (DERP) map that the tailnet actually uses.
//!
//! Headscale's API has no DERP surface, and its config only names the *sources*
//! of the map: fetch URLs, local map files, and an optional embedded relay. This
//! module turns those sources into a region list, so the UI can show which relay
//! servers exist instead of only where they are configured from.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use serde_yaml_ng::Value as YamlValue;

/// The time limit for a remote map fetch.
const FETCH_TIMEOUT: Duration = Duration::from_secs(5);

/// One relay region and the servers in it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Region {
    pub id: i64,
    pub code: String,
    pub name: String,
    /// Where this region's definition came from (`url:…`, `file:…`, `embedded`).
    pub source: String,
    pub nodes: Vec<RelayServer>,
}

/// One relay server in a region.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RelayServer {
    pub name: String,
    pub hostname: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stun_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub derp_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipv4: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipv6: Option<String>,
}

/// The configured sources of the DERP map.
#[derive(Debug, Clone, Default)]
pub struct Sources {
    pub urls: Vec<String>,
    pub paths: Vec<String>,
    pub embedded: Option<Embedded>,
}

/// The embedded relay server's identity, from `derp.server`.
#[derive(Debug, Clone)]
pub struct Embedded {
    pub region_id: i64,
    pub region_code: String,
    pub region_name: String,
    pub stun_listen_addr: String,
}

/// Resolves every source into one region list. It collects failures per
/// source, so one unreachable URL does not hide the rest.
pub async fn fetch(sources: &Sources) -> (Vec<Region>, Vec<String>) {
    let mut by_id: BTreeMap<i64, Region> = BTreeMap::new();
    let mut errors = Vec::new();

    if let Some(embedded) = &sources.embedded {
        merge(&mut by_id, vec![embedded_region(embedded)]);
    }

    if !sources.urls.is_empty() {
        let client = reqwest::Client::builder()
            .timeout(FETCH_TIMEOUT)
            .build()
            .expect("building the DERP map client");
        for url in &sources.urls {
            match client.get(url).send().await {
                Ok(response) => match response.json::<Value>().await {
                    Ok(value) => merge(&mut by_id, parse_map(&value, &format!("url:{url}"))),
                    Err(err) => errors.push(format!("{url}: {err}")),
                },
                Err(err) => errors.push(format!("{url}: {err}")),
            }
        }
    }

    for path in &sources.paths {
        match tokio::fs::read_to_string(path).await {
            Ok(text) => match serde_yaml_ng::from_str::<YamlValue>(&text) {
                Ok(value) => merge(&mut by_id, parse_map(&yaml_to_json(value), &format!("file:{path}"))),
                Err(err) => errors.push(format!("{path}: {err}")),
            },
            Err(err) => errors.push(format!("{path}: {err}")),
        }
    }

    (by_id.into_values().collect(), errors)
}

/// Merges regions into the accumulator and deduplicates servers by hostname.
fn merge(by_id: &mut BTreeMap<i64, Region>, regions: Vec<Region>) {
    for region in regions {
        match by_id.get_mut(&region.id) {
            Some(existing) => {
                for node in region.nodes {
                    if !existing.nodes.iter().any(|have| have.hostname == node.hostname) {
                        existing.nodes.push(node);
                    }
                }
            }
            None => {
                by_id.insert(region.id, region);
            }
        }
    }
}

fn embedded_region(embedded: &Embedded) -> Region {
    // `stun_listen_addr` is a `host:port`. The port is what the UI shows.
    let stun_port = embedded
        .stun_listen_addr
        .rsplit(':')
        .next()
        .and_then(|port| port.trim().parse().ok());

    Region {
        id: embedded.region_id,
        code: embedded.region_code.clone(),
        name: embedded.region_name.clone(),
        source: "embedded".into(),
        nodes: vec![RelayServer {
            name: embedded.region_code.clone(),
            hostname: "(embedded DERP server)".into(),
            stun_port,
            derp_port: None,
            ipv4: None,
            ipv6: None,
        }],
    }
}

/// Parses a DERP map. Accepts both the JSON shape Tailscale serves (`Regions`
/// with `RegionID`/`HostName`) and Headscale's YAML shape (`regions` with
/// `regionid`/`hostname`). The lookups are case-insensitive.
fn parse_map(value: &Value, source: &str) -> Vec<Region> {
    let Some(regions) = get_ci(value, "regions") else {
        return Vec::new();
    };

    match regions {
        Value::Object(map) => map.values().filter_map(|region| parse_region(region, source)).collect(),
        Value::Array(items) => items.iter().filter_map(|region| parse_region(region, source)).collect(),
        _ => Vec::new(),
    }
}

fn parse_region(value: &Value, source: &str) -> Option<Region> {
    let id = get_ci(value, "regionid").and_then(as_i64)?;
    let code = get_ci(value, "regioncode")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let name = get_ci(value, "regionname")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let nodes = get_ci(value, "nodes")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_node).collect())
        .unwrap_or_default();

    Some(Region {
        id,
        code,
        name,
        source: source.to_string(),
        nodes,
    })
}

fn parse_node(value: &Value) -> Option<RelayServer> {
    let hostname = get_ci(value, "hostname").and_then(Value::as_str)?.to_string();
    let name = get_ci(value, "name")
        .and_then(Value::as_str)
        .unwrap_or(&hostname)
        .to_string();

    Some(RelayServer {
        name,
        hostname,
        stun_port: get_ci(value, "stunport").and_then(as_u16),
        derp_port: get_ci(value, "derpport").and_then(as_u16),
        ipv4: get_ci(value, "ipv4").and_then(Value::as_str).map(str::to_string),
        ipv6: get_ci(value, "ipv6").and_then(Value::as_str).map(str::to_string),
    })
}

/// Case-insensitive object lookup: `RegionID`, `regionid` and `RegionId` all
/// resolve.
fn get_ci<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    value
        .as_object()?
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value)
}

fn as_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str()?.trim().parse().ok())
}

fn as_u16(value: &Value) -> Option<u16> {
    value
        .as_u64()
        .and_then(|number| u16::try_from(number).ok())
        .or_else(|| value.as_str()?.trim().parse().ok())
}

/// Converts a YAML value to JSON. Map keys become strings (YAML region keys
/// are integers, which JSON objects cannot hold).
fn yaml_to_json(value: YamlValue) -> Value {
    match value {
        YamlValue::Null => Value::Null,
        YamlValue::Bool(flag) => Value::Bool(flag),
        YamlValue::Number(number) => {
            if let Some(int) = number.as_i64() {
                Value::Number(int.into())
            } else if let Some(uint) = number.as_u64() {
                Value::Number(uint.into())
            } else if let Some(float) = number.as_f64() {
                serde_json::Number::from_f64(float)
                    .map(Value::Number)
                    .unwrap_or(Value::Null)
            } else {
                Value::Null
            }
        }
        YamlValue::String(text) => Value::String(text),
        YamlValue::Sequence(items) => Value::Array(items.into_iter().map(yaml_to_json).collect()),
        YamlValue::Mapping(map) => {
            let mut object = serde_json::Map::new();
            for (key, item) in map {
                let key = match key {
                    YamlValue::String(text) => text,
                    other => serde_yaml_ng::to_string(&other)
                        .unwrap_or_default()
                        .trim()
                        .to_string(),
                };
                object.insert(key, yaml_to_json(item));
            }
            Value::Object(object)
        }
        YamlValue::Tagged(tagged) => yaml_to_json(tagged.value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_tailscale_json_map() {
        let value: Value = serde_json::from_str(
            r#"{
                "Regions": {
                    "1": {
                        "RegionID": 1,
                        "RegionCode": "nyc",
                        "RegionName": "New York City",
                        "Nodes": [
                            { "Name": "1a", "HostName": "derp1.tailscale.com", "STUNPort": 3478, "DERPPort": 443 }
                        ]
                    }
                }
            }"#,
        )
        .unwrap();

        let regions = parse_map(&value, "url:test");
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].code, "nyc");
        assert_eq!(regions[0].nodes[0].hostname, "derp1.tailscale.com");
        assert_eq!(regions[0].nodes[0].stun_port, Some(3478));
        assert_eq!(regions[0].nodes[0].derp_port, Some(443));
    }

    #[test]
    fn parses_headscale_yaml_with_integer_region_keys() {
        let yaml: YamlValue = serde_yaml_ng::from_str(
            r#"
regions:
  2:
    regionid: 2
    regioncode: sin
    regionname: Singapore
    nodes:
      - name: 2a
        hostname: derp2.example.com
        stunport: 3478
"#,
        )
        .unwrap();

        let regions = parse_map(&yaml_to_json(yaml), "file:test");
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].id, 2);
        assert_eq!(regions[0].name, "Singapore");
        assert_eq!(regions[0].nodes[0].hostname, "derp2.example.com");
    }

    #[test]
    fn merges_regions_from_multiple_sources() {
        let mut by_id = BTreeMap::new();
        merge(
            &mut by_id,
            vec![Region {
                id: 1,
                code: "nyc".into(),
                name: "New York".into(),
                source: "url:a".into(),
                nodes: vec![RelayServer {
                    name: "1a".into(),
                    hostname: "a.example".into(),
                    stun_port: None,
                    derp_port: None,
                    ipv4: None,
                    ipv6: None,
                }],
            }],
        );
        merge(
            &mut by_id,
            vec![Region {
                id: 1,
                code: "nyc".into(),
                name: "New York".into(),
                source: "file:b".into(),
                nodes: vec![
                    RelayServer {
                        name: "1a".into(),
                        hostname: "a.example".into(),
                        stun_port: None,
                        derp_port: None,
                        ipv4: None,
                        ipv6: None,
                    },
                    RelayServer {
                        name: "1b".into(),
                        hostname: "b.example".into(),
                        stun_port: None,
                        derp_port: None,
                        ipv4: None,
                        ipv6: None,
                    },
                ],
            }],
        );

        assert_eq!(by_id.len(), 1);
        assert_eq!(by_id[&1].nodes.len(), 2, "duplicate hostnames collapse");
    }

    #[test]
    fn embedded_region_reads_the_stun_port() {
        let region = embedded_region(&Embedded {
            region_id: 999,
            region_code: "self".into(),
            region_name: "Embedded".into(),
            stun_listen_addr: "0.0.0.0:3478".into(),
        });
        assert_eq!(region.nodes[0].stun_port, Some(3478));
        assert_eq!(region.source, "embedded");
    }
}
