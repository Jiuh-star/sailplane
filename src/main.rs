//! Sailplane: a web UI for Headscale.

mod acl;
mod agent;
mod auth;
mod config;
mod db;
mod headscale;
mod hsconfig;
mod integrations;
mod ssh;
mod unix_socket;
mod util;
mod web;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

/// How often expired sessions are swept from the database.
const SESSION_SWEEP_INTERVAL: Duration = Duration::from_secs(15 * 60);

#[derive(Parser, Debug)]
#[command(
    name = "sailplane",
    version,
    about = "A feature-complete web UI for Headscale"
)]
struct Cli {
    /// Path to the configuration file.
    #[arg(
        long,
        env = "SAILPLANE_CONFIG_PATH",
        default_value = "/etc/sailplane/config.yaml"
    )]
    config: PathBuf,

    /// Validate the configuration and exit.
    #[arg(long)]
    check: bool,

    /// Print the effective configuration and exit.
    #[arg(long)]
    show_config: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Load configuration before installing the logger, so a config error is
    // reported plainly rather than through a half-configured subscriber.
    let config = match config::load_from(&cli.config) {
        Ok(config) => Arc::new(config),
        Err(err) => {
            init_logging(false);
            eprintln!("sailplane: {err:#}");
            std::process::exit(1);
        }
    };

    init_logging(config.debug_logging());

    if cli.show_config {
        println!(
            "{}",
            serde_yaml_ng::to_string(&*config).context("failed to render the configuration")?
        );
        return Ok(());
    }

    if cli.check {
        println!("configuration is valid");
        return Ok(());
    }

    if let Err(err) = run(config).await {
        tracing::error!("{err:#}");
        return Err(err);
    }
    Ok(())
}

fn init_logging(debug: bool) {
    let default_level = if debug { "debug" } else { "info" };
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("sailplane={default_level},warn")));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}

async fn run(config: Arc<config::Config>) -> Result<()> {
    let prefix = config.server.base_path();
    tracing::info!(
        "starting sailplane {} (headscale: {})",
        env!("CARGO_PKG_VERSION"),
        config.headscale.url
    );

    // --- Persistence ---
    let db_path = config.server.data_path.join("sailplane_persist.db");
    let db = db::Db::open(&db_path)
        .with_context(|| format!("failed to open the database at {}", db_path.display()))?;
    tracing::info!("using database {}", db_path.display());

    // --- Headscale ---
    let headscale = headscale::Headscale::new(
        &config.headscale.url,
        config.headscale.tls_cert_path.as_deref(),
    )?;

    match headscale.probe_version().await {
        Ok(Some(version)) if version.below_minimum() => tracing::error!(
            "headscale {} is older than the minimum supported version {}; some features \
             will not work",
            version.raw,
            headscale::version::MIN_SUPPORTED
        ),
        Ok(Some(version)) => tracing::info!("connected to headscale {}", version.raw),
        Ok(None) => tracing::error!(
            "headscale /version returned 404; version {} or newer is required",
            headscale::version::MIN_SUPPORTED
        ),
        Err(err) => tracing::warn!(
            "could not determine the headscale version ({err:#}); assuming newest capabilities"
        ),
    }
    // Keep retrying until a version is known, so an in-place upgrade is picked
    // up without restarting Sailplane.
    if headscale.version().unknown {
        headscale.spawn_version_poller();
    }

    // --- Live snapshots ---
    let live = headscale::LiveStore::new();
    {
        let headscale = headscale.clone();
        let api_key = config.headscale.api_key.clone();
        live.spawn_pollers(move || api_key.clone().map(|key| headscale.client(key)));
    }

    // --- Headscale config file ---
    let hsconfig = hsconfig::HeadscaleConfigFile::load(
        config.headscale.config_path.as_deref(),
        config.headscale.dns_records_path.as_deref(),
    )?;
    match hsconfig.access() {
        hsconfig::ConfigAccess::No => tracing::warn!(
            "no readable Headscale configuration file; DNS and authentication restriction \
             editing is disabled"
        ),
        hsconfig::ConfigAccess::ReadOnly => {
            tracing::warn!("the Headscale configuration file is read-only")
        }
        hsconfig::ConfigAccess::ReadWrite => {
            tracing::info!("Headscale configuration is writable")
        }
    }

    // --- Integrations ---
    let integration = integrations::Integration::from_config(config.integration.as_ref());
    tracing::info!("reload integration: {}", integration.name());

    // --- Agent ---
    let mut agent_config = config
        .integration
        .as_ref()
        .and_then(|integration| integration.agent.clone())
        .unwrap_or_else(|| config::AgentConfig {
            enabled: false,
            host_name: "sailplane-agent".into(),
            cache_ttl: 180_000,
            backend: config::AgentBackend::System,
            socket: None,
            executable_path: PathBuf::from("/usr/libexec/sailplane/agent"),
            work_dir: config.server.data_path.join("agent"),
            tailscale_netns: true,
        });

    if agent_config.socket.is_none() {
        agent_config.socket = agent::tailscale::default_socket();
    }
    let agent_disabled_reason = agent_disabled_reason(config.as_ref(), &headscale);
    if agent_disabled_reason.is_none() {
        match agent::tailscale::probe(agent_config.socket.as_deref()).await {
            Some(source) => tracing::info!("agent host info source: {}", source.as_str()),
            None => tracing::warn!(
                "the Tailscale daemon did not answer; host info will fall back to the command \
                 line, which cannot report peer versions"
            ),
        }
    }
    let agent = Arc::new(agent::AgentService::new(
        db.clone(),
        agent_config,
        agent_disabled_reason,
        config
            .headscale
            .api_key
            .clone()
            .map(|key| headscale.client(key)),
    ));
    agent.spawn_refresher(live.clone());

    // --- Browser SSH ---
    let ssh_config = config
        .integration
        .as_ref()
        .and_then(|integration| integration.ssh.clone())
        .unwrap_or_default();
    let ssh_disabled_reason = ssh_disabled_reason(&ssh_config);
    let ssh = ssh::SshService::new(ssh_config, ssh_disabled_reason);

    // --- Auth ---
    let auth = auth::AuthService::new(db.clone(), config.clone());

    // --- OIDC ---
    let oidc = match config.oidc.as_ref() {
        Some(oidc_config) if oidc_config.enabled => {
            match auth::oidc::OidcProvider::new(oidc_config.clone()) {
                Ok(provider) => {
                    // Warm discovery so the login page knows whether SSO works.
                    match provider.discover().await {
                        Ok(_) => tracing::info!("OpenID Connect provider is ready"),
                        Err(err) => tracing::warn!(
                            "OpenID Connect discovery failed ({}); single sign-on stays \
                             unavailable until the provider responds",
                            err.message()
                        ),
                    }
                    Some(provider)
                }
                Err(err) => {
                    tracing::error!("failed to initialise OpenID Connect: {err:#}");
                    None
                }
            }
        }
        _ => None,
    };

    // --- Server ---
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    let state = Arc::new(web::state::AppState {
        config: config.clone(),
        db: db.clone(),
        auth,
        headscale,
        live,
        hsconfig,
        integration,
        agent: (*agent).clone(),
        ssh,
        oidc,
        shutdown: shutdown_rx,
    });

    spawn_session_sweeper(db);

    let app = web::router(state);
    let address: SocketAddr = format!("{}:{}", config.server.host, config.server.port)
        .parse()
        .with_context(|| {
            format!(
                "invalid listen address {}:{}",
                config.server.host, config.server.port
            )
        })?;

    let scheme = if config.server.tls_enabled() {
        "https"
    } else {
        "http"
    };

    let listener = TcpListener::bind(address)
        .await
        .with_context(|| format!("failed to bind {address}"))?;
    let bound = listener.local_addr()?;

    write_listen_file(config.as_ref(), scheme, bound.port());
    tracing::info!("listening on {scheme}://{bound}{prefix}/");

    // End the long-lived streams first, then let in-flight requests finish.
    let shutdown = async move {
        shutdown_signal().await;
        let _ = shutdown_tx.send(true);
    };

    if config.server.tls_enabled() {
        serve_tls(listener, app, config.as_ref(), shutdown).await
    } else {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown)
        .await
        .context("server error")
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.ok();
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }

    tracing::info!("shutting down");
}

/// Explains why the agent cannot run, or `None` when it can.
fn agent_disabled_reason(
    config: &config::Config,
    headscale: &headscale::Headscale,
) -> Option<String> {
    let agent_config = config.integration.as_ref()?.agent.as_ref()?;
    if !agent_config.enabled {
        return Some("The Sailplane agent is disabled in the configuration.".into());
    }
    if config.headscale.api_key.is_none() {
        return Some(
            "The agent needs `headscale.api_key` so it can register itself and read node data."
                .into(),
        );
    }
    if !headscale.capabilities().agent_supported() {
        return Some(
            "The agent requires Headscale 0.28.0 or newer for tag-only pre-authentication keys."
                .into(),
        );
    }
    if agent_config.backend == config::AgentBackend::System {
        let socket = agent_config
            .socket
            .clone()
            .unwrap_or_else(|| std::path::PathBuf::from(agent::tailscale::DEFAULT_SOCKET));
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

/// Explains why browser SSH cannot run, or `None` when it can.
fn ssh_disabled_reason(config: &config::SshConfig) -> Option<String> {
    if !config.enabled {
        return None;
    }
    // A proxy is how Sailplane reaches a tailnet it is not itself a member of;
    // without one, direct connectivity is assumed.
    if let Some(proxy) = config.proxy.as_deref() {
        let address = proxy
            .trim_start_matches("socks5h://")
            .trim_start_matches("socks5://")
            .trim_start_matches("socks://");
        if std::net::TcpStream::connect_timeout(
            &address
                .parse()
                .unwrap_or_else(|_| "127.0.0.1:0".parse().expect("valid literal")),
            std::time::Duration::from_millis(500),
        )
        .is_err()
        {
            return Some(format!(
                "The configured SSH proxy at {address} is not reachable. If you rely on the \
                 sidecar tailscaled, set `TS_SOCKS5_SERVER` so it exposes one."
            ));
        }
    }
    None
}

/// Writes the loopback health URL for container health checks.
fn write_listen_file(config: &config::Config, scheme: &str, port: u16) {
    let Ok(path) = std::env::var("SAILPLANE_LISTEN_FILE") else {
        return;
    };
    let url = format!(
        "{scheme}://127.0.0.1:{port}{}/healthz",
        config.server.base_path()
    );
    if let Err(err) = std::fs::write(&path, &url) {
        tracing::warn!("could not write the listen file at {path}: {err}");
    }
}

fn spawn_session_sweeper(db: db::Db) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(SESSION_SWEEP_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            match db.prune_expired_sessions() {
                Ok(0) => {}
                Ok(removed) => tracing::debug!("pruned {removed} expired sessions"),
                Err(err) => tracing::warn!("failed to prune sessions: {err:#}"),
            }
        }
    });
}

async fn serve_tls(
    listener: TcpListener,
    app: axum::Router,
    config: &config::Config,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> Result<()> {
    use axum_server::tls_rustls::RustlsConfig;

    let cert_path = config
        .server
        .tls_cert_path
        .as_ref()
        .context("tls_cert_path is required for TLS")?;
    let key_path = config
        .server
        .tls_key_path
        .as_ref()
        .context("tls_key_path is required for TLS")?;

    let tls = RustlsConfig::from_pem_file(cert_path, key_path)
        .await
        .with_context(|| {
            format!(
                "failed to load the TLS certificate {} and key {}",
                cert_path.display(),
                key_path.display()
            )
        })?;

    let address = listener.local_addr()?;
    drop(listener);

    // `axum_server` drives shutdown through a handle rather than a future.
    let handle = axum_server::Handle::new();
    let shutdown_handle = handle.clone();
    tokio::spawn(async move {
        shutdown.await;
        shutdown_handle.graceful_shutdown(Some(Duration::from_secs(10)));
    });

    axum_server::bind_rustls(address, tls)
        .handle(handle)
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .context("TLS server error")
}
