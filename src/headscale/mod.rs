//! Headscale API access: wire types, version capabilities, the HTTP client and
//! the SSE snapshot store.

pub mod client;
pub mod live;
pub mod types;
pub mod version;

pub use client::{ApiClient, Headscale, HeadscaleError};
pub use live::LiveStore;
pub use types::*;
pub use version::{Capabilities, ServerVersion};
