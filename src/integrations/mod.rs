//! Reload integrations.
//!
//! DNS and authentication-restriction edits change the Headscale config file on
//! disk; Headscale only picks those up on restart or `SIGHUP`. Exactly one
//! integration may be enabled, and it is invoked after every config write.

pub mod docker;
pub mod kubernetes;
pub mod process;

use std::sync::Arc;

use anyhow::Result;

use crate::config::IntegrationConfig;
use crate::headscale::Headscale;

use docker::DockerIntegration;
use kubernetes::KubernetesIntegration;

/// The single configured reload mechanism.
#[derive(Clone)]
pub enum Integration {
    Docker(Arc<DockerIntegration>),
    Kubernetes(Arc<KubernetesIntegration>),
    Proc,
    /// No integration configured: config edits are written but Headscale is
    /// not reloaded automatically.
    None,
}

impl Integration {
    /// Streams the Headscale container's logs, when the enabled integration
    /// can reach them.
    pub async fn log_stream(
        &self,
        tail: usize,
        follow: bool,
    ) -> Result<futures_util::stream::BoxStream<'static, Result<hyper::body::Bytes, std::io::Error>>> {
        match self {
            Self::Docker(docker) => docker.log_stream(tail, follow).await,
            Self::None => anyhow::bail!(
                "no reload integration is configured, so Sailplane cannot read the \
                 Headscale logs. Enable `integration.docker`."
            ),
            _ => anyhow::bail!(
                "the configured reload integration cannot read container logs; only \
                 `integration.docker` can"
            ),
        }
    }

    /// Builds the integration described by the configuration, validating
    /// availability up front so the UI can warn early.
    pub fn from_config(config: Option<&IntegrationConfig>) -> Self {
        let Some(config) = config else {
            return Self::None;
        };

        if let Some(docker) = config.docker.as_ref().filter(|c| c.enabled) {
            return Self::Docker(Arc::new(DockerIntegration::new(docker.clone())));
        }

        if let Some(kubernetes) = config.kubernetes.as_ref().filter(|c| c.enabled) {
            let integration = KubernetesIntegration::new(kubernetes.clone());
            if !integration.is_available() {
                tracing::warn!(
                    "the kubernetes integration is enabled but no in-cluster service account \
                     was found at {}",
                    kubernetes::SERVICE_ACCOUNT_DIR
                );
            }
            return Self::Kubernetes(Arc::new(integration));
        }

        if config.proc.as_ref().is_some_and(|c| c.enabled) {
            return Self::Proc;
        }

        Self::None
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Docker(_) => "docker",
            Self::Kubernetes(_) => "kubernetes",
            Self::Proc => "proc",
            Self::None => "none",
        }
    }

    pub fn is_enabled(&self) -> bool {
        !matches!(self, Self::None)
    }

    /// Reloads Headscale so it picks up configuration changes.
    ///
    /// A failure here does not lose the edit: the file is already written.
    /// Callers surface it as a warning rather than rolling back.
    pub async fn on_config_change(&self, headscale: &Headscale) -> Result<()> {
        match self {
            Self::Docker(integration) => integration.on_config_change(headscale).await,
            Self::Kubernetes(integration) => integration.on_config_change(headscale).await,
            Self::Proc => {
                let pid = process::find_headscale_pid(std::path::Path::new("/proc")).ok_or_else(
                    || {
                        anyhow::anyhow!(
                            "could not find a running `headscale serve` process in /proc"
                        )
                    },
                )?;
                process::reload_via_signal(pid, headscale).await
            }
            Self::None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DockerConfig;

    #[test]
    fn no_config_yields_none() {
        assert_eq!(Integration::from_config(None).name(), "none");
        assert!(!Integration::from_config(None).is_enabled());
    }

    #[test]
    fn docker_is_selected_when_enabled() {
        let config = IntegrationConfig {
            docker: Some(DockerConfig {
                enabled: true,
                container_name: None,
                container_label: "sailplane.target=headscale".into(),
                socket: "unix:///var/run/docker.sock".into(),
            }),
            kubernetes: None,
            proc: None,
            agent: None,
            ssh: None,
        };
        assert_eq!(Integration::from_config(Some(&config)).name(), "docker");
    }

    #[test]
    fn proc_is_selected_when_enabled() {
        let config = IntegrationConfig {
            docker: None,
            kubernetes: None,
            proc: Some(crate::config::integration::ProcConfig { enabled: true }),
            agent: None,
            ssh: None,
        };
        assert_eq!(Integration::from_config(Some(&config)).name(), "proc");
    }

    #[test]
    fn disabled_sections_are_ignored() {
        let config = IntegrationConfig {
            docker: Some(DockerConfig {
                enabled: false,
                container_name: None,
                container_label: "x".into(),
                socket: "unix:///var/run/docker.sock".into(),
            }),
            kubernetes: None,
            proc: None,
            agent: None,
            ssh: None,
        };
        assert_eq!(Integration::from_config(Some(&config)).name(), "none");
    }
}
