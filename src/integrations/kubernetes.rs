//! Kubernetes integration.
//!
//! Sailplane runs as a sidecar in the same pod as Headscale and reloads it by
//! signalling the `headscale serve` PID found in `/proc`. This requires
//! `shareProcessNamespace: true` on the pod, which is validated up front using
//! the in-cluster service account.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::config::KubernetesConfig;
use crate::headscale::Headscale;

use super::process::{find_headscale_pid, reload_via_signal};

/// Where Kubernetes mounts the service account credentials.
pub const SERVICE_ACCOUNT_DIR: &str = "/var/run/secrets/kubernetes.io/serviceaccount";

/// `/proc` mount used to discover the Headscale process.
const PROC_ROOT: &str = "/proc";

pub struct KubernetesIntegration {
    config: KubernetesConfig,
    service_account_dir: PathBuf,
    proc_root: PathBuf,
}

impl KubernetesIntegration {
    pub fn new(config: KubernetesConfig) -> Self {
        Self {
            config,
            service_account_dir: PathBuf::from(SERVICE_ACCOUNT_DIR),
            proc_root: PathBuf::from(PROC_ROOT),
        }
    }
    /// Overrides where credentials and `/proc` are read from. Test-only.
    #[cfg(test)]
    pub fn with_paths(
        mut self,
        service_account_dir: &std::path::Path,
        proc_root: &std::path::Path,
    ) -> Self {
        self.service_account_dir = service_account_dir.to_path_buf();
        self.proc_root = proc_root.to_path_buf();
        self
    }

    /// True when the in-cluster service account is present.
    pub fn is_available(&self) -> bool {
        self.service_account_dir.join("token").is_file()
    }

    pub async fn on_config_change(&self, headscale: &Headscale) -> Result<()> {
        if self.config.validate_manifest {
            self.validate_share_process_namespace().await?;
        }

        let pid = find_headscale_pid(&self.proc_root).context(
            "could not find a running `headscale serve` process; the pod must set \
             `spec.shareProcessNamespace: true` for Sailplane to reload it",
        )?;

        reload_via_signal(pid, headscale).await
    }

    /// Confirms the pod shares its process namespace, so the `/proc` scan can
    /// see Headscale.
    async fn validate_share_process_namespace(&self) -> Result<()> {
        let token = self.read_service_account_file("token")?;
        let namespace = self.read_service_account_file("namespace")?;
        let Some(pod_name) = self.config.pod_name.clone() else {
            bail!("integration.kubernetes.pod_name is required to validate the pod manifest");
        };

        let client = self.build_client()?;
        let url = format!(
            "https://kubernetes.default.svc/api/v1/namespaces/{namespace}/pods/{pod_name}"
        );

        let response = client
            .get(&url)
            .bearer_auth(token.trim())
            .header("Accept", "application/json")
            .send()
            .await
            .context("failed to reach the Kubernetes API")?;

        if response.status() == reqwest::StatusCode::FORBIDDEN {
            bail!(
                "the service account is not allowed to read pod `{pod_name}`; grant `get` on pods \
                 or set `integration.kubernetes.validate_manifest: false`"
            );
        }
        if !response.status().is_success() {
            bail!(
                "the Kubernetes API returned {} while reading pod `{pod_name}`",
                response.status()
            );
        }

        let pod: Value = response
            .json()
            .await
            .context("failed to parse the pod manifest")?;

        let shared = pod
            .pointer("/spec/shareProcessNamespace")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        if !shared {
            bail!(
                "pod `{pod_name}` does not set `spec.shareProcessNamespace: true`, so Sailplane \
                 cannot see the Headscale process"
            );
        }

        Ok(())
    }

    fn read_service_account_file(&self, name: &str) -> Result<String> {
        let path = self.service_account_dir.join(name);
        std::fs::read_to_string(&path).with_context(|| {
            format!(
                "missing Kubernetes service account file {}; is Sailplane running in-cluster?",
                path.display()
            )
        })
    }

    fn build_client(&self) -> Result<reqwest::Client> {
        let mut builder = reqwest::Client::builder().timeout(Duration::from_secs(10));

        let ca_path = self.service_account_dir.join("ca.crt");
        if let Ok(pem) = std::fs::read(&ca_path) {
            match reqwest::Certificate::from_pem(&pem) {
                Ok(cert) => builder = builder.add_root_certificate(cert),
                Err(err) => tracing::warn!("could not parse {}: {err}", ca_path.display()),
            }
        }

        builder.build().context("failed to build the Kubernetes HTTP client")
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use std::io::Write as _;

    fn integration(dir: &Path, validate: bool) -> KubernetesIntegration {
        KubernetesIntegration::new(KubernetesConfig {
            enabled: true,
            pod_name: Some("headscale".into()),
            validate_manifest: validate,
        })
        .with_paths(dir, Path::new("/proc"))
    }

    #[test]
    fn reports_unavailable_without_a_service_account() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!integration(dir.path(), true).is_available());
    }

    #[test]
    fn reports_available_with_a_token_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut token = std::fs::File::create(dir.path().join("token")).unwrap();
        token.write_all(b"abc").unwrap();
        assert!(integration(dir.path(), true).is_available());
    }

    #[tokio::test]
    async fn missing_service_account_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let headscale = Headscale::new("http://127.0.0.1:1", None).unwrap();
        let err = integration(dir.path(), true)
            .on_config_change(&headscale)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("service account"), "{err}");
    }
}
