//! Read/write access to the Headscale configuration file.
//!
//! Writes go through [`YamlEditor`], which patches the document in place so
//! comments, ordering and formatting survive. Every mutation is serialized
//! through a mutex and followed by an integration reload (`on_config_change`),
//! because Headscale only re-reads its config on SIGHUP or restart.

pub mod yaml_edit;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;

use yaml_edit::{Path as EditPath, YamlEditor, parse_path};

/// How much of the Headscale configuration Sailplane can change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigAccess {
    /// No `headscale.config_path` configured, or the file is unreadable.
    No,
    /// Readable but not writable: the UI is read-only.
    ReadOnly,
    /// Readable and writable: config-backed features are fully enabled.
    ReadWrite,
}

impl ConfigAccess {
    pub fn readable(self) -> bool {
        matches!(self, Self::ReadOnly | Self::ReadWrite)
    }

    pub fn writable(self) -> bool {
        self == Self::ReadWrite
    }
}

/// A DNS record from `dns.extra_records` or the extra-records JSON file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DnsRecord {
    pub name: String,

    #[serde(rename = "type")]
    pub record_type: String,

    pub value: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DnsConfig {
    pub magic_dns: bool,
    pub base_domain: Option<String>,
    /// Nameservers applied to the whole tailnet.
    pub nameservers: Vec<String>,
    /// Per-domain nameservers, split DNS.
    pub split_dns: BTreeMap<String, Vec<String>>,
    pub search_domains: Vec<String>,
    pub override_dns: bool,
    pub extra_records: Vec<DnsRecord>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OidcRestrictions {
    pub issuer: Option<String>,
    pub allowed_domains: Vec<String>,
    pub allowed_groups: Vec<String>,
    pub allowed_users: Vec<String>,
}

/// Handle to the Headscale config file (and optional extra-records file).
#[derive(Clone)]
pub struct HeadscaleConfigFile {
    inner: Arc<Inner>,
}

struct Inner {
    config_path: Option<PathBuf>,
    records_path: Option<PathBuf>,
    access: ConfigAccess,
    /// Serializes writes so two concurrent patches cannot interleave.
    lock: Mutex<()>,
}

impl HeadscaleConfigFile {
    /// Resolves the config file paths and detects read/write access.
    ///
    /// `override_records_path` is Sailplane's `dns_records_path`. It wins over
    /// the path in the Headscale config, because the two mount points can
    /// differ between containers.
    pub fn load(config_path: Option<&Path>, override_records_path: Option<&Path>) -> Result<Self> {
        let access = match config_path {
            Some(path) if path.is_file() => {
                if std::fs::OpenOptions::new().write(true).open(path).is_ok() {
                    ConfigAccess::ReadWrite
                } else {
                    ConfigAccess::ReadOnly
                }
            }
            Some(path) => {
                tracing::warn!(
                    "headscale.config_path {} does not exist. Config-backed features are disabled",
                    path.display()
                );
                ConfigAccess::No
            }
            None => ConfigAccess::No,
        };

        // Parse the Headscale config once. The records path and the
        // "advertises" check below both read `dns.extra_records_path`.
        let parsed_config = match (access.readable(), config_path) {
            (true, Some(path)) => serde_yaml_ng::from_str::<serde_yaml_ng::Value>(
                &std::fs::read_to_string(path).unwrap_or_default(),
            )
            .ok(),
            _ => None,
        };
        let configured_records_path = parsed_config
            .as_ref()
            .and_then(|value| value.get("dns"))
            .and_then(|dns| dns.get("extra_records_path"))
            .and_then(|path| path.as_str())
            .filter(|path| !path.is_empty());

        // The extra-records file wins over an inline `extra_records` list. A
        // relative path resolves against the config file's directory, not the
        // process working directory.
        let mut records_path = override_records_path.map(Path::to_path_buf);
        if records_path.is_none()
            && let Some(raw_path) = configured_records_path
        {
            let path = PathBuf::from(raw_path);
            records_path = Some(
                match (path.is_absolute(), config_path.and_then(Path::parent)) {
                    (false, Some(dir)) => dir.join(path),
                    _ => path,
                },
            );
        }

        if let (Some(override_path), Some(config)) = (override_records_path, config_path) {
            // Upstream refuses to start when Sailplane is told about a records
            // file that Headscale itself does not know about.
            if configured_records_path.is_none() {
                bail!(
                    "sailplane.dns_records_path is set to {} but the Headscale config at {} does \
                     not set dns.extra_records_path. Headscale would ignore the file",
                    override_path.display(),
                    config.display()
                );
            }
        }

        Ok(Self {
            inner: Arc::new(Inner {
                config_path: config_path.map(Path::to_path_buf),
                records_path,
                access,
                lock: Mutex::new(()),
            }),
        })
    }

    pub fn access(&self) -> ConfigAccess {
        self.inner.access
    }

    pub fn readable(&self) -> bool {
        self.inner.access.readable()
    }

    pub fn writable(&self) -> bool {
        self.inner.access.writable()
    }
    fn read(&self) -> Result<String> {
        let Some(path) = self.inner.config_path.as_deref() else {
            bail!("no Headscale configuration file is configured");
        };
        std::fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))
    }

    pub fn document(&self) -> Result<YamlEditor> {
        Ok(YamlEditor::new(self.read()?))
    }

    // --- DNS ---

    pub fn dns_config(&self) -> Result<DnsConfig> {
        let editor = self.document()?;

        let split_dns = {
            let value = editor.get_value(&parse_path("dns.nameservers.split"));
            match value {
                Some(Value::Object(map)) => map
                    .into_iter()
                    .map(|(domain, servers)| {
                        let servers = match servers {
                            Value::Array(items) => items
                                .into_iter()
                                .filter_map(|item| item.as_str().map(str::to_string))
                                .collect(),
                            _ => Vec::new(),
                        };
                        (domain, servers)
                    })
                    .collect(),
                _ => BTreeMap::new(),
            }
        };

        let mut extra_records = self
            .records_from_file()?
            .unwrap_or_else(|| self.records_from_config(&editor));

        extra_records.sort_by(|a, b| a.name.cmp(&b.name).then(a.record_type.cmp(&b.record_type)));

        Ok(DnsConfig {
            magic_dns: editor
                .get_bool(&parse_path("dns.magic_dns"))
                .unwrap_or(true),
            base_domain: editor.get_str(&parse_path("dns.base_domain")),
            nameservers: editor.get_string_list(&parse_path("dns.nameservers.global")),
            split_dns,
            search_domains: editor.get_string_list(&parse_path("dns.search_domains")),
            override_dns: editor
                .get_bool(&parse_path("dns.override_local_dns"))
                .unwrap_or(true),
            extra_records,
        })
    }

    fn records_from_config(&self, editor: &YamlEditor) -> Vec<DnsRecord> {
        match editor.get_value(&parse_path("dns.extra_records")) {
            Some(Value::Array(items)) => items
                .into_iter()
                .filter_map(|item| serde_json::from_value(item).ok())
                .collect(),
            _ => Vec::new(),
        }
    }

    fn records_from_file(&self) -> Result<Option<Vec<DnsRecord>>> {
        let Some(path) = self.inner.records_path.as_deref() else {
            return Ok(None);
        };
        if !path.is_file() {
            return Ok(Some(Vec::new()));
        }
        let raw = std::fs::read_to_string(path).with_context(|| {
            format!("failed to read the DNS records file at {}", path.display())
        })?;
        let records = serde_json::from_str(&raw).with_context(|| {
            format!("failed to parse the DNS records file at {}", path.display())
        })?;
        Ok(Some(records))
    }

    /// Adds a DNS record and deduplicates on name + type.
    pub async fn add_dns_record(&self, record: DnsRecord) -> Result<()> {
        self.mutate_records(move |records| {
            if records.iter().any(|existing| {
                existing.name == record.name && existing.record_type == record.record_type
            }) {
                bail!(
                    "a {} record for {} already exists",
                    record.record_type,
                    record.name
                );
            }
            records.push(record);
            Ok(())
        })
        .await
    }

    pub async fn remove_dns_record(&self, record: &DnsRecord) -> Result<()> {
        self.mutate_records(|records| {
            records.retain(|existing| {
                !(existing.name == record.name && existing.record_type == record.record_type)
            });
            Ok(())
        })
        .await
    }

    /// Applies `mutate` to the record list under the write lock, so two
    /// concurrent edits cannot lose an update.
    async fn mutate_records<F>(&self, mutate: F) -> Result<()>
    where
        F: FnOnce(&mut Vec<DnsRecord>) -> Result<()>,
    {
        let _guard = self.inner.lock.lock().await;
        let mut records = self.current_records()?;
        let before = records.len();
        mutate(&mut records)?;
        if records.len() == before {
            return Ok(());
        }
        self.write_records(records)
    }

    fn current_records(&self) -> Result<Vec<DnsRecord>> {
        if self.inner.records_path.is_some() {
            Ok(self.records_from_file()?.unwrap_or_default())
        } else {
            let editor = self.document()?;
            Ok(self.records_from_config(&editor))
        }
    }

    /// Writes the record list. The caller must hold the write lock.
    fn write_records(&self, records: Vec<DnsRecord>) -> Result<()> {
        if !self.inner.access.writable() {
            bail!("the Headscale configuration is read-only. DNS records cannot be changed");
        }

        if let Some(path) = self.inner.records_path.clone() {
            let json = serde_json::to_string_pretty(&records)?;
            write_atomically(&path, &json)?;
            return Ok(());
        }

        // No separate file: the records live inside the config document.
        self.patch_locked(&[(
            parse_path("dns.extra_records"),
            Some(serde_json::to_value(&records)?),
        )])
    }

    // --- OIDC restrictions ---

    pub fn oidc_restrictions(&self) -> Result<OidcRestrictions> {
        let editor = self.document()?;
        Ok(OidcRestrictions {
            issuer: editor.get_str(&parse_path("oidc.issuer")),
            allowed_domains: dedupe(editor.get_string_list(&parse_path("oidc.allowed_domains"))),
            allowed_groups: dedupe(editor.get_string_list(&parse_path("oidc.allowed_groups"))),
            allowed_users: dedupe(editor.get_string_list(&parse_path("oidc.allowed_users"))),
        })
    }

    // --- Generic patching ---

    /// Applies a batch of `path -> value` changes (`None` deletes the key).
    pub async fn patch(&self, changes: &[(EditPath, Option<Value>)]) -> Result<()> {
        let _guard = self.inner.lock.lock().await;
        self.patch_locked(changes)
    }

    fn patch_locked(&self, changes: &[(EditPath, Option<Value>)]) -> Result<()> {
        let Some(path) = self.inner.config_path.clone() else {
            bail!("no Headscale configuration file is configured");
        };
        if !self.inner.access.writable() {
            bail!("the Headscale configuration file is not writable");
        }

        let mut editor = YamlEditor::new(std::fs::read_to_string(&path)?);
        for (key_path, value) in changes {
            match value {
                Some(value) => editor.set(key_path, value.clone())?,
                None => editor.remove(key_path)?,
            }
        }

        // Refuse to write a document that no longer parses.
        serde_yaml_ng::from_str::<serde_yaml_ng::Value>(editor.as_str())
            .context("refusing to write an invalid Headscale configuration")?;

        write_atomically(&path, editor.as_str())
    }
}

fn dedupe(mut values: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    values.retain(|value| seen.insert(value.clone()));
    values
}

/// Writes a file by replacing it atomically, so a crash cannot truncate the
/// Headscale configuration.
fn write_atomically(path: &Path, contents: &str) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let temp = parent.join(format!(
        ".{}.sailplane-tmp",
        path.file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "config".into())
    ));

    std::fs::write(&temp, contents)
        .with_context(|| format!("failed to write {}", temp.display()))?;
    std::fs::rename(&temp, path)
        .with_context(|| format!("failed to replace {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn temp_config(contents: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(contents.as_bytes()).unwrap();
        (dir, path)
    }

    const CONFIG: &str = r#"# headscale config
dns:
  magic_dns: true
  base_domain: old.example.com
  nameservers:
    global:
      - 1.1.1.1
  search_domains:
    - corp.example
policy:
  mode: database
"#;

    #[test]
    fn reads_dns_config() {
        let (_dir, path) = temp_config(CONFIG);
        let file = HeadscaleConfigFile::load(Some(&path), None).unwrap();
        assert_eq!(file.access(), ConfigAccess::ReadWrite);

        let dns = file.dns_config().unwrap();
        assert!(dns.magic_dns);
        assert_eq!(dns.base_domain.as_deref(), Some("old.example.com"));
        assert_eq!(dns.nameservers, vec!["1.1.1.1"]);
        assert_eq!(dns.search_domains, vec!["corp.example"]);
    }

    #[test]
    fn missing_config_is_disabled() {
        let file =
            HeadscaleConfigFile::load(Some(Path::new("/nonexistent/config.yaml")), None).unwrap();
        assert_eq!(file.access(), ConfigAccess::No);
        assert!(!file.readable());
    }

    #[test]
    fn no_config_path_is_disabled() {
        let file = HeadscaleConfigFile::load(None, None).unwrap();
        assert_eq!(file.access(), ConfigAccess::No);
    }

    #[tokio::test]
    async fn patching_preserves_comments() {
        let (_dir, path) = temp_config(CONFIG);
        let file = HeadscaleConfigFile::load(Some(&path), None).unwrap();

        file.patch(&[(
            parse_path("dns.base_domain"),
            Some(Value::String("new.example.com".into())),
        )])
        .await
        .unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# headscale config"));
        assert!(text.contains("base_domain: new.example.com"));
        assert!(text.contains("policy:\n  mode: database"));
    }

    #[tokio::test]
    async fn patch_errors_when_not_writable() {
        use std::os::unix::fs::PermissionsExt;

        let (_dir, path) = temp_config(CONFIG);
        // Make the file read-only for the current user.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();

        let file = HeadscaleConfigFile::load(Some(&path), None).unwrap();
        // Running as root bypasses the permission bits. Skip in that case.
        if file.access() == ConfigAccess::ReadWrite {
            return;
        }
        assert_eq!(file.access(), ConfigAccess::ReadOnly);

        let err = file
            .patch(&[(
                parse_path("dns.base_domain"),
                Some(Value::String("x".into())),
            )])
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not writable"));
    }

    #[tokio::test]
    async fn records_in_config_file_round_trip() {
        let (_dir, path) = temp_config(CONFIG);
        let file = HeadscaleConfigFile::load(Some(&path), None).unwrap();

        file.add_dns_record(DnsRecord {
            name: "git.example.com".into(),
            record_type: "A".into(),
            value: "100.64.0.5".into(),
        })
        .await
        .unwrap();

        let dns = file.dns_config().unwrap();
        assert_eq!(dns.extra_records.len(), 1);
        assert_eq!(dns.extra_records[0].value, "100.64.0.5");

        // Duplicates are refused.
        assert!(
            file.add_dns_record(DnsRecord {
                name: "git.example.com".into(),
                record_type: "A".into(),
                value: "100.64.0.6".into(),
            })
            .await
            .is_err()
        );

        file.remove_dns_record(&DnsRecord {
            name: "git.example.com".into(),
            record_type: "A".into(),
            value: "100.64.0.5".into(),
        })
        .await
        .unwrap();
        assert!(file.dns_config().unwrap().extra_records.is_empty());
    }

    #[tokio::test]
    async fn records_in_separate_json_file() {
        let (dir, path) = temp_config("dns:\n  extra_records_path: /tmp/records.json\n");
        let records_path = dir.path().join("records.json");
        std::fs::write(&records_path, "[]").unwrap();

        let file = HeadscaleConfigFile::load(Some(&path), Some(&records_path)).unwrap();

        file.add_dns_record(DnsRecord {
            name: "a.example.com".into(),
            record_type: "AAAA".into(),
            value: "fd7a::1".into(),
        })
        .await
        .unwrap();

        let raw = std::fs::read_to_string(&records_path).unwrap();
        assert!(raw.contains("a.example.com"));
        // The config file itself is untouched.
        assert!(
            !std::fs::read_to_string(&path)
                .unwrap()
                .contains("a.example.com")
        );
    }

    #[tokio::test]
    async fn a_corrupt_records_file_is_an_error() {
        let (dir, path) = temp_config("dns:\n  extra_records_path: records.json\n");
        let records_path = dir.path().join("records.json");
        std::fs::write(&records_path, "{not json").unwrap();

        let file = HeadscaleConfigFile::load(Some(&path), Some(&records_path)).unwrap();
        assert!(file.dns_config().is_err());
        assert!(
            file.add_dns_record(DnsRecord {
                name: "a.example.com".into(),
                record_type: "A".into(),
                value: "100.64.0.5".into(),
            })
            .await
            .is_err()
        );
    }

    #[tokio::test]
    async fn records_writes_are_refused_when_read_only() {
        use std::os::unix::fs::PermissionsExt;

        let (dir, path) = temp_config("dns:\n  extra_records_path: records.json\n");
        let records_path = dir.path().join("records.json");
        std::fs::write(&records_path, "[]").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();

        let file = HeadscaleConfigFile::load(Some(&path), Some(&records_path)).unwrap();
        // Running as root bypasses the permission bits. Skip in that case.
        if file.access() == ConfigAccess::ReadWrite {
            return;
        }

        let err = file
            .add_dns_record(DnsRecord {
                name: "a.example.com".into(),
                record_type: "A".into(),
                value: "100.64.0.5".into(),
            })
            .await
            .unwrap_err();
        assert!(err.to_string().contains("read-only"));
        assert_eq!(std::fs::read_to_string(&records_path).unwrap(), "[]");
    }

    #[test]
    fn a_relative_records_path_resolves_against_the_config_file() {
        let (dir, path) = temp_config("dns:\n  extra_records_path: records.json\n");
        let file = HeadscaleConfigFile::load(Some(&path), None).unwrap();
        assert_eq!(
            file.inner.records_path.as_deref(),
            Some(dir.path().join("records.json").as_path())
        );
    }

    #[test]
    fn refuses_records_path_absent_from_headscale_config() {
        let (_dir, path) = temp_config("dns:\n  magic_dns: true\n");
        let err = HeadscaleConfigFile::load(Some(&path), Some(Path::new("/tmp/records.json")))
            .err()
            .expect("a records path unknown to Headscale must be rejected");
        assert!(err.to_string().contains("dns.extra_records_path"));
    }

    /// Headscale refuses to start when `override_local_dns` is true and the
    /// global list is empty, so the DNS handlers guard against writing that.
    /// This pins the shape the guard looks for.
    #[test]
    fn detects_an_empty_global_nameserver_list() {
        let (_dir, path) =
            temp_config("dns:\n  override_local_dns: true\n  nameservers:\n    global: []\n");
        let file = HeadscaleConfigFile::load(Some(&path), None).unwrap();
        let dns = file.dns_config().unwrap();
        assert!(dns.nameservers.is_empty());
        assert!(dns.override_dns);
    }

    #[test]
    fn reads_oidc_restrictions() {
        let (_dir, path) = temp_config(
            r#"oidc:
  issuer: https://idp.example
  allowed_domains:
    - corp.example
  allowed_users:
    - alice
"#,
        );
        let file = HeadscaleConfigFile::load(Some(&path), None).unwrap();
        let oidc = file.oidc_restrictions().unwrap();
        assert_eq!(oidc.issuer.as_deref(), Some("https://idp.example"));
        assert_eq!(oidc.allowed_domains, vec!["corp.example"]);
        assert_eq!(oidc.allowed_users, vec!["alice"]);
        assert!(oidc.allowed_groups.is_empty());
    }
}
