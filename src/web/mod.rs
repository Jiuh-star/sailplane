//! HTTP surface: the JSON API consumed by the SPA plus a handful of
//! integration endpoints (`/healthz`, `/api/info`, `/events/live`, OIDC).

pub mod acl;
pub mod auth_routes;
pub mod error;
pub mod live;
pub mod machines;
pub mod middleware;
pub mod presentation;
pub mod settings;
pub mod setup;
pub mod ssh;
pub mod state;
pub mod static_files;
pub mod topology;
pub mod users;
pub mod util;

use axum::Router;
use axum::routing::{delete, get, post, put};
use tower_http::limit::RequestBodyLimitLayer;

use state::SharedState;

/// Largest JSON body accepted. Policies and configs are small; this keeps a
/// hostile client from forcing large allocations.
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;

/// Builds the router, mounted under the configured base path.
pub fn router(state: SharedState) -> Router {
    let api = Router::new()
        // --- Session and auth ---
        .route("/session", get(auth_routes::session))
        .route("/auth/login", post(auth_routes::login))
        .route("/auth/logout", post(auth_routes::logout))
        // --- Machines ---
        .route("/machines", get(machines::list))
        .route("/machines/register", post(machines::register))
        .route("/machines/{id}", get(machines::detail))
        .route("/machines/{id}", delete(machines::remove))
        .route("/machines/{id}/rename", post(machines::rename))
        .route("/machines/{id}/expire", post(machines::expire))
        .route("/machines/{id}/expiry", post(machines::toggle_expiry))
        .route("/machines/{id}/tags", post(machines::update_tags))
        .route("/machines/{id}/routes", post(machines::update_routes))
        .route("/machines/{id}/owner", post(machines::reassign))
        // --- Headscale users ---
        .route("/users", get(users::list))
        .route("/users", post(users::create))
        .route("/users/{id}", delete(users::remove))
        .route("/users/{id}/rename", post(users::rename))
        .route("/users/{name}/groups", put(users::update_groups))
        // --- Sailplane accounts ---
        .route("/accounts/{id}/role", post(users::reassign_role))
        .route("/accounts/{id}/link", post(users::link))
        .route(
            "/accounts/{id}/transfer-ownership",
            post(users::transfer_ownership),
        )
        .route("/accounts/{id}", delete(users::delete_account))
        // --- Access control ---
        .route("/acl", get(acl::get_policy))
        .route("/acl", put(acl::set_policy))
        .route("/acl/simulate", post(acl::simulate))
        // --- DNS ---
        .route("/derp", get(settings::derp::get))
        .route("/derp", post(settings::derp::update))
        .route("/dns", get(settings::dns::get_config))
        .route("/dns/tailnet", post(settings::dns::rename_tailnet))
        .route("/dns/magic", post(settings::dns::toggle_magic))
        .route("/dns/nameservers", post(settings::dns::add_nameserver))
        .route(
            "/dns/nameservers/remove",
            post(settings::dns::remove_nameserver),
        )
        .route(
            "/dns/search-domains",
            post(settings::dns::add_search_domain),
        )
        .route(
            "/dns/search-domains/remove",
            post(settings::dns::remove_search_domain),
        )
        .route("/dns/records", post(settings::dns::add_record))
        .route("/dns/records/remove", post(settings::dns::remove_record))
        .route("/dns/override", post(settings::dns::set_override))
        // --- Settings ---
        .route("/auth-keys", get(settings::auth_keys::list))
        .route("/auth-keys", post(settings::auth_keys::create))
        .route("/auth-keys/expire", post(settings::auth_keys::expire))
        .route("/auth-keys/delete", post(settings::auth_keys::delete))
        .route("/api-keys", get(settings::api_keys::list))
        .route("/api-keys/revoke", post(settings::api_keys::revoke))
        .route("/topology", get(topology::get))
        .route("/audit", get(settings::audit::list))
        .route("/logs", get(settings::logs::stream))
        .route("/oidc", get(settings::oidc::get))
        .route("/oidc", post(settings::oidc::update))
        .route("/restrictions", get(settings::restrictions::get))
        .route("/restrictions", post(settings::restrictions::update))
        .route("/agent", get(settings::agent::status))
        .route("/agent/sync", post(settings::agent::sync))
        // --- Sailplane's own settings ---
        .route("/settings", get(settings::sailplane::get))
        .route("/settings", put(settings::sailplane::update))
        .route("/settings/validate", post(settings::sailplane::validate))
        .route("/settings/import", post(settings::sailplane::import))
        // --- First-run onboarding (public until complete) ---
        .route("/setup/status", get(setup::status))
        .route("/setup/test-headscale", post(setup::test_headscale))
        .route("/setup/complete", post(setup::complete))
        // --- Browser SSH ---
        .route("/ssh/{id}", get(ssh::info))
        .route("/ssh/{id}/ws", get(ssh::connect));

    let app = Router::new()
        .route("/", get(static_files::serve))
        .route("/healthz", get(util::healthz))
        .route("/api/info", get(util::info))
        .route("/api/color-scheme", post(util::color_scheme))
        .route("/events/live", get(live::stream))
        .route("/oidc/start", get(auth_routes::oidc_start))
        .route("/oidc/callback", get(auth_routes::oidc_callback))
        .nest("/api", api)
        .fallback(static_files::serve);

    let prefix = state.prefix().to_string();
    let app = if prefix.is_empty() {
        app
    } else {
        // `nest` does not forward the bare `{prefix}/` request to the inner
        // router, so the mount root needs an explicit route here. The layers
        // are applied afterwards so that route gets them as well.
        Router::new()
            .route(&format!("{prefix}/"), get(static_files::serve))
            .nest(&prefix, app)
    };

    let audited = state.clone();
    app.layer(axum::middleware::from_fn_with_state(
        audited,
        middleware::audit,
    ))
    .layer(axum::middleware::from_fn(middleware::security_headers))
    .layer(axum::middleware::from_fn(middleware::origin_check))
    .layer(RequestBodyLimitLayer::new(MAX_BODY_BYTES))
    .with_state(state)
}
