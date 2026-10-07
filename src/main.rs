//! Sailplane: a web UI for Headscale.

mod acl;
mod agent;
mod auth;
mod config;
mod db;
mod derp;
mod headscale;
mod hsconfig;
mod integrations;
mod ssh;
mod unix_socket;
mod util;
mod web;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
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
    /// Legacy configuration file. Sailplane imports it into the database on
    /// first start, then deprecates it.
    #[arg(
        long,
        env = "SAILPLANE_CONFIG_PATH",
        default_value = "/etc/sailplane/config.yaml"
    )]
    config: PathBuf,

    /// Import a configuration file into the database and exit.
    #[arg(long)]
    import_config: Option<PathBuf>,

    /// Validate the configuration and exit.
    #[arg(long)]
    check: bool,

    /// Print the effective configuration and exit.
    #[arg(long)]
    show_config: bool,

    /// Reveal secret values in `--show-config` output.
    #[arg(long)]
    show_secrets: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    init_logging(config::env_flag("SAILPLANE_DEBUG_LOG") || config::env_flag("SAILPLANE_DEBUG"));

    let data_path = boot_data_path(&cli.config);
    let db_path = data_path.join("sailplane_persist.db");
    let db = match db::Db::open(&db_path) {
        Ok(db) => db,
        Err(err) => {
            eprintln!(
                "sailplane: failed to open the database at {}: {err:#}",
                db_path.display()
            );
            std::process::exit(1);
        }
    };

    // The legacy file is an import source only. `--config` still works for a
    // one-time migration. A missing file is not an error.
    let legacy = cli.config.exists().then_some(cli.config.as_path());
    let settings = match config::store::Settings::bootstrap(&db, legacy, Some(&data_path)) {
        Ok(settings) => settings,
        Err(err) => {
            eprintln!("sailplane: {err:#}");
            std::process::exit(1);
        }
    };

    if let Some(path) = cli.import_config.as_deref() {
        config::import::import_file(&db, path)
            .with_context(|| format!("failed to import {}", path.display()))?;
        settings.reload(&db)?;
        println!("imported {} into the database", path.display());
        return Ok(());
    }

    if cli.show_config {
        println!("{}", render_config(&settings.snapshot(), cli.show_secrets)?);
        return Ok(());
    }

    if cli.check {
        println!("configuration is valid");
        return Ok(());
    }

    if let Err(err) = run(settings, db).await {
        tracing::error!("{err:#}");
        return Err(err);
    }
    Ok(())
}

/// The data directory. Sailplane must know it before it reads the database,
/// and thus before it reads any stored setting.
///
/// An environment variable wins. Otherwise Sailplane uses `server.data_path`
/// from a legacy config file, so an existing deployment upgrades in place
/// without extra flags.
fn boot_data_path(legacy: &Path) -> PathBuf {
    if let Ok(value) = std::env::var("SAILPLANE_DATA_PATH") {
        return PathBuf::from(value);
    }
    if let Ok(value) = std::env::var("SAILPLANE_SERVER__DATA_PATH") {
        return PathBuf::from(value);
    }
    if let Ok(raw) = std::fs::read_to_string(legacy)
        && let Ok(value) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&raw)
        && let Some(path) = value
            .get("server")
            .and_then(|server| server.get("data_path"))
            .and_then(|path| path.as_str())
    {
        return PathBuf::from(path);
    }
    PathBuf::from("/var/lib/sailplane/")
}

/// Renders the configuration as YAML and masks secret values unless
/// `show_secrets` is true.
fn render_config(config: &config::Config, show_secrets: bool) -> Result<String> {
    let mut value =
        serde_yaml_ng::to_value(config).context("failed to render the configuration")?;
    if !show_secrets {
        mask_secrets(&mut value, &mut Vec::new());
    }
    serde_yaml_ng::to_string(&value).context("failed to render the configuration")
}

fn mask_secrets(value: &mut serde_yaml_ng::Value, path: &mut Vec<String>) {
    match value {
        serde_yaml_ng::Value::Mapping(map) => {
            for (key, child) in map.iter_mut() {
                let Some(key) = key.as_str() else { continue };
                path.push(key.to_string());
                let dotted = path.join(".");
                if config::schema::descriptor(&dotted).is_some_and(|entry| entry.secret) {
                    *child = serde_yaml_ng::Value::String("***".into());
                } else {
                    mask_secrets(child, path);
                }
                path.pop();
            }
        }
        serde_yaml_ng::Value::Sequence(seq) => {
            for child in seq.iter_mut() {
                mask_secrets(child, path);
            }
        }
        _ => {}
    }
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

async fn run(settings: config::store::Settings, db: db::Db) -> Result<()> {
    let config = settings.snapshot();
    let prefix = config.server.base_path().to_string();
    tracing::info!(
        "starting sailplane {} (headscale: {})",
        env!("CARGO_PKG_VERSION"),
        config.headscale.url
    );
    tracing::info!("using database {}", config.server.data_path.display());

    // --- Headscale ---
    let headscale = headscale::Headscale::new(
        &config.headscale.url,
        config.headscale.tls_cert_path.as_deref(),
    )?;

    match headscale.probe_version().await {
        Ok(Some(version)) if version.below_minimum() => tracing::error!(
            "headscale {} is older than the minimum supported version {}. Some features \
             will not work",
            version.raw,
            headscale::version::MIN_SUPPORTED
        ),
        Ok(Some(version)) => tracing::info!("connected to headscale {}", version.raw),
        Ok(None) => tracing::error!(
            "headscale /version returned 404. Version {} or newer is required",
            headscale::version::MIN_SUPPORTED
        ),
        Err(err) => tracing::warn!(
            "could not determine the headscale version ({err:#}). Sailplane assumes the \
             newest capabilities"
        ),
    }
    // Keep retrying until the version is known, so Sailplane picks up an
    // in-place upgrade without a restart.
    if headscale.version().unknown {
        headscale.spawn_version_poller();
    }

    // --- Live snapshots ---
    let live = headscale::LiveStore::new();
    {
        let headscale = headscale.clone();
        // Read the key and URL from the settings store each tick, so rotating
        // the API key takes effect without a restart.
        let settings = settings.clone();
        live.spawn_pollers(move || {
            let config = settings.snapshot();
            // Keep the client pointed at the configured URL, in case it changed
            // since the last tick.
            headscale.set_base_url(&config.headscale.url);
            config
                .headscale
                .api_key
                .clone()
                .map(|key| headscale.client(key))
        });
    }

    // --- Headscale config file ---
    let hsconfig = hsconfig::HeadscaleConfigFile::load(
        config.headscale.config_path.as_deref(),
        config.headscale.dns_records_path.as_deref(),
    )?;
    match hsconfig.access() {
        hsconfig::ConfigAccess::No => tracing::warn!(
            "no readable Headscale configuration file. DNS and authentication restriction \
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
    let agent_config = config.agent_config();
    let agent_disabled_reason = agent::disabled_reason(config.as_ref(), &headscale);
    if agent_disabled_reason.is_none() {
        match agent::tailscale::probe(agent_config.socket.as_deref()).await {
            Some(source) => tracing::info!("agent host info source: {}", source.as_str()),
            None => tracing::warn!(
                "the Tailscale daemon did not answer. Host info will fall back to the command \
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
    let ssh_config = config.ssh_config();
    let ssh_disabled_reason = ssh::disabled_reason(&ssh_config);
    let ssh = ssh::SshService::new(ssh_config, ssh_disabled_reason);

    // --- Auth ---
    let auth = auth::AuthService::new(db.clone(), settings.clone());

    // --- OIDC ---
    let oidc = match config.oidc.as_ref() {
        Some(oidc_config) if oidc_config.enabled => {
            match auth::oidc::OidcProvider::new(oidc_config.clone()) {
                Ok(provider) => {
                    // Warm discovery so the login page knows whether SSO works.
                    match provider.discover().await {
                        Ok(_) => tracing::info!("OpenID Connect provider is ready"),
                        Err(err) => tracing::warn!(
                            "OpenID Connect discovery failed ({}). Single sign-on stays \
                             unavailable until the provider responds",
                            err.message()
                        ),
                    }
                    Some(provider)
                }
                Err(err) => {
                    tracing::error!("failed to initialize OpenID Connect: {err:#}");
                    None
                }
            }
        }
        _ => None,
    };

    // --- Server ---
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    let state = Arc::new(web::state::AppState {
        settings: settings.clone(),
        prefix: prefix.clone(),
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
