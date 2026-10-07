//! Settings endpoints: DNS, pre-auth keys, Headscale API keys, authentication
//! restrictions and the agent.

pub mod agent;
pub mod api_keys;
pub mod audit;
pub mod auth_keys;
pub mod derp;
pub mod dns;
pub mod logs;
pub mod oidc;
pub mod restrictions;
pub mod sailplane;

use super::state::SharedState;

/// Reloads Headscale after a configuration file change.
///
/// The edit is already on disk when this runs, so a reload failure is a warning,
/// not an error. The save succeeded, and the UI must still refresh the list.
pub async fn reload_after_change(state: &SharedState) -> Option<String> {
    if !state.integration.is_enabled() {
        tracing::warn!(
            "the Headscale configuration changed but no reload integration is configured; \
             restart Headscale (or send it SIGHUP) for the change to take effect"
        );
        return Some(
            "Saved, but no reload integration is configured. Restart Headscale to apply it."
                .into(),
        );
    }

    match state.integration.on_config_change(&state.headscale).await {
        Ok(()) => None,
        Err(err) => {
            tracing::warn!("failed to reload Headscale after a config change: {err:#}");
            Some(format!(
                "Saved, but Headscale could not be reloaded automatically ({}). Restart it to \
                 apply the change.",
                state.integration.name()
            ))
        }
    }
}
