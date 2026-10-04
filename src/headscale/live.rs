//! Cached Headscale snapshots with change notifications.
//!
//! One shared client polls nodes every 5 s and users every 15 s, as upstream
//! does. A changed payload bumps a version counter; browsers subscribe over
//! SSE and refetch only when their resource's version moves. Snapshots stay in
//! memory, so a page load never blocks on Headscale.

use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tokio::sync::{RwLock, broadcast};

use super::client::ApiClient;
use super::types::{Machine, User};

/// Resources exposed to the live stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKey {
    Nodes,
    Users,
}

impl ResourceKey {
    /// Returns the poll interval for the background refresher.
    fn interval(self) -> Duration {
        match self {
            Self::Nodes => Duration::from_secs(5),
            Self::Users => Duration::from_secs(15),
        }
    }
}

/// An immutable snapshot of one resource plus the version it was stored at.
#[derive(Debug, Clone)]
pub struct Snapshot<T> {
    pub data: Arc<T>,
    pub version: u64,
}

/// A change event broadcast to SSE subscribers.
#[derive(Debug, Clone, Serialize)]
pub struct ChangeEvent {
    pub resource: ResourceKey,
    pub version: u64,
}

/// In-memory snapshot store shared by every request.
pub struct LiveStore {
    nodes: RwLock<Snapshot<Vec<Machine>>>,
    users: RwLock<Snapshot<Vec<User>>>,
    changes: broadcast::Sender<ChangeEvent>,
}

/// Default type parameters cannot express `Vec<Machine>::default` through the
/// generic helper, so the two snapshots get explicit constructors.
impl LiveStore {
    pub fn new() -> Arc<Self> {
        let (changes, _) = broadcast::channel(256);
        Arc::new(Self {
            nodes: RwLock::new(Snapshot {
                data: Arc::new(Vec::new()),
                version: 0,
            }),
            users: RwLock::new(Snapshot {
                data: Arc::new(Vec::new()),
                version: 0,
            }),
            changes,
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ChangeEvent> {
        self.changes.subscribe()
    }

    pub async fn nodes(&self) -> Snapshot<Vec<Machine>> {
        self.nodes.read().await.clone()
    }

    pub async fn users(&self) -> Snapshot<Vec<User>> {
        self.users.read().await.clone()
    }

    pub fn nodes_version_blocking(&self) -> u64 {
        self.nodes
            .try_read()
            .map(|snapshot| snapshot.version)
            .unwrap_or(0)
    }

    pub fn users_version_blocking(&self) -> u64 {
        self.users
            .try_read()
            .map(|snapshot| snapshot.version)
            .unwrap_or(0)
    }

    /// Replaces the node snapshot. The version bumps only when the payload
    /// changed, so idle pollers do not spam subscribers.
    pub async fn set_nodes(&self, nodes: Vec<Machine>) {
        let mut guard = self.nodes.write().await;
        if versions_equal(guard.data.as_ref(), &nodes) {
            return;
        }
        guard.version += 1;
        guard.data = Arc::new(nodes);
        let version = guard.version;
        drop(guard);
        let _ = self.changes.send(ChangeEvent {
            resource: ResourceKey::Nodes,
            version,
        });
    }

    pub async fn set_users(&self, users: Vec<User>) {
        let mut guard = self.users.write().await;
        if versions_equal(guard.data.as_ref(), &users) {
            return;
        }
        guard.version += 1;
        guard.data = Arc::new(users);
        let version = guard.version;
        drop(guard);
        let _ = self.changes.send(ChangeEvent {
            resource: ResourceKey::Users,
            version,
        });
    }
    /// Spawns one polling task per resource.
    ///
    /// The client is re-read from `client_provider` on every tick. A rotated
    /// API key or a late version probe then takes effect without a restart. A
    /// provider that returns `None` (no API key configured) skips the tick.
    pub fn spawn_pollers<F>(self: &Arc<Self>, client_provider: F)
    where
        F: Fn() -> Option<ApiClient> + Send + Sync + 'static,
    {
        let provider = Arc::new(client_provider);

        for resource in [ResourceKey::Nodes, ResourceKey::Users] {
            let store = Arc::clone(self);
            let provider = Arc::clone(&provider);

            tokio::spawn(async move {
                let mut interval = tokio::time::interval(resource.interval());
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

                loop {
                    interval.tick().await;

                    let Some(client) = provider() else {
                        continue;
                    };

                    match resource {
                        ResourceKey::Nodes => match client.list_nodes().await {
                            Ok(nodes) => store.set_nodes(nodes).await,
                            Err(err) => {
                                tracing::debug!("node snapshot refresh failed: {err}")
                            }
                        },
                        ResourceKey::Users => match client.list_users().await {
                            Ok(users) => store.set_users(users).await,
                            Err(err) => {
                                tracing::debug!("user snapshot refresh failed: {err}")
                            }
                        },
                    }
                }
            });
        }
    }
}

/// Compares two payloads without requiring `PartialEq` on the wire types.
fn versions_equal<T: Serialize>(a: &T, b: &T) -> bool {
    match (serde_json::to_value(a), serde_json::to_value(b)) {
        (Ok(a), Ok(b)) => a == b,
        // If serialisation fails, treat the payload as changed so the UI is
        // never stuck on a stale snapshot.
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::headscale::types::Machine;

    #[tokio::test]
    async fn version_bumps_only_on_change() {
        let store = LiveStore::new();
        let node: Machine = serde_json::from_str(r#"{"id":"1","givenName":"a"}"#).unwrap();

        store.set_nodes(vec![node.clone()]).await;
        assert_eq!(store.nodes().await.version, 1);

        // Identical payload: no bump.
        store.set_nodes(vec![node.clone()]).await;
        assert_eq!(store.nodes().await.version, 1);

        store.set_nodes(vec![]).await;
        assert_eq!(store.nodes().await.version, 2);
    }

    #[tokio::test]
    async fn subscribers_receive_change_events() {
        let store = LiveStore::new();
        let mut rx = store.subscribe();
        store
            .set_users(vec![serde_json::from_str(r#"{"id":"1","name":"u"}"#).unwrap()])
            .await;

        let event = rx.recv().await.unwrap();
        assert_eq!(event.resource, ResourceKey::Users);
        assert_eq!(event.version, 1);
    }
}
