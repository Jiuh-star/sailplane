# Architecture

Sailplane is a web UI for [Headscale](https://headscale.net). One Rust binary
serves a Vue 3 single-page application, a JSON API, and a server-sent event
stream. The browser talks to the same origin; there is no server-side
rendering.

## Design constraints

The process must stay below 50 MB RSS with every feature enabled. Measured with
the agent, browser SSH, SSE and the Docker integration on: about 16 MB RSS and a
12.6 MB binary.

The design avoids runtime allocation for static content:

- The frontend is embedded in the binary at compile time with `rust-embed`.
- Assets are pre-compressed at build time into `.gz` and `.br` siblings. The
  server picks one from `Accept-Encoding` and sends it as-is. There is no
  runtime compression.
- SQLite runs on one connection with a 2 MB page cache.
- The release profile uses `opt-level = "s"`, thin LTO, and one codegen unit.
- The Tokio runtime uses a small worker pool.
- The API is same-origin, so the server has no CORS layer.

Vue Flow and CodeMirror load only with the routes that use them.

## Modules

```
src/
  config/         settings store (defaults + database + environment), schema, import
  headscale/      API client, wire types, version capabilities, live snapshot store
  auth/           sessions, roles and capabilities, OIDC, proxy auth
  db/             SQLite schema and queries
  acl/            HuJSON parsing and the structured policy model
  hsconfig/       comment-preserving YAML editing of the Headscale config
  integrations/   docker / kubernetes / proc reload strategies
  agent/          host info collection from the Tailscale daemon
  ssh/            server-side SSH client for browser SSH
  web/            HTTP surface: routes, SSE, static assets
```

## Request flow

1. The SPA loads from the embedded bundle. Session state comes from the
   `_sailplane_auth` cookie.
2. API handlers call the Headscale v1 API through `headscale::client`. The
   client sends `Authorization: Bearer <key>` and a `Sailplane/<version>`
   user agent.
3. A background store polls Headscale for nodes every 5 seconds and users every
   15 seconds. Changes broadcast to every open browser over
   `GET <base_path>/events/live` as `changed` events, with a heartbeat every
   15 seconds.
4. State-changing requests must pass an origin check. The terminal WebSocket
   upgrade checks the origin too, because CORS never sees WebSocket handshakes.

## Configuration and boot

Sailplane's own settings are stored in SQLite and edited from the UI
(`config/schema.rs` describes each field; `config/store.rs` builds the effective
configuration from defaults, then the database, then environment overrides). The
store holds a swappable snapshot, so a save takes effect per request; fields that
cannot hot-apply (listener, TLS, base path, integrations) are marked
**restart required** in the schema.

Boot order: read the data directory (environment, else a legacy config file) so
the database can be opened, import a legacy config file if the settings table is
empty, generate the cookie secret and setup token if absent, build the snapshot,
then wire the services and bind. A legacy YAML file is migration input only.

## Version capabilities

Sailplane targets Headscale 0.27.0 and newer. It reads the server version at
startup and derives a capability set from it. Examples: `preAuthKeysHaveStableIds`
needs 0.28.0, `keyExpiryCanBeDisabled` needs 0.29.0. An unparseable version (a
`dev` build, for example) enables every capability.

A 404 from `GET /version` means Headscale is older than 0.27.0. The service logs
an error and keeps running. Headscale 0.29.0 and 0.29.1 have a `/ts2021`
regression; the client works around it.

## The Go components, replaced

Upstream Headplane ships two Go programs. Sailplane replaces both with one
rootless `tailscaled` sidecar and no custom binaries.

| Upstream | Sailplane |
| --- | --- |
| `hp_agent`, a `tsnet` node that reports per-peer host info | The server reads the sidecar's LocalAPI netmap: `POST /localapi/v0/debug?action=current-netmap`. `Peers[].Hostinfo` carries the client version, OS, endpoints, NetInfo and SSH host keys. `Peers[].Key` is the `nodeKey` Headscale reports, so the two join directly. It falls back to `GET /localapi/v0/status`, then the `tailscale status --json` command line. |
| `hp_ssh`, a WASM Tailscale node in the browser | The server is the SSH client. It opens a session with `russh` through the sidecar's SOCKS5 proxy (`TS_SOCKS5_SERVER`) and bridges the terminal to xterm.js over a WebSocket. |

The agent needs debug access to the LocalAPI: run as root, or run
`tailscale set --operator` once. Without it, the agent falls back to the status
projection. That projection has every field except the client version, and the
log says which source it used.

The `external` agent backend can drive an upstream `hp_agent` binary instead.
That binary reads the `HEADPLANE_AGENT_*` environment variables, which is why
those names stay in the source.

## Security position

Sailplane adds these protections over upstream:

- An origin check on every state-changing request, and on the terminal
  WebSocket upgrade.
- A content security policy with `frame-ancestors 'none'`, plus the usual
  security headers.
- The session cookie is `HttpOnly`.
- Browser SSH requires write access to the machine. The server connects with its
  own tailnet identity, so the human at the terminal is not authenticated to the
  target. Read-only roles get no shell.
- `oidc.disable_api_key_login` is enforced by the API, not only hidden in the UI.
- Config edits that would stop Headscale from starting are refused, not written.
- Values that reach the Headscale config file are validated and escaped. A user
  id from a URL is resolved against the live user list, never interpolated into
  a request path.
- The Headscale client ignores `HTTP_PROXY` and `HTTPS_PROXY`, so the API key
  cannot be routed through an intermediary.
- The audit log never stores the query string, which can carry a key or token.
  It keeps the most recent 5000 entries.

## Behaviour notes

- A config edit that Headscale cannot reload is a warning, not an error. The
  file is already written, so the API answers 200 with a `warning` field.
- The DNS handlers reject edits Headscale would refuse at startup, such as an
  empty `dns.nameservers.global` with `override_local_dns: true`.
- The structured policy round-trips byte-identically. `serde_json` is built with
  `preserve_order`, so the API keeps declaration order and the text diff stays
  readable.
- The audit log lives in the same SQLite database as the accounts and sessions.
  Headscale keeps no history of its own, so it is the only record of who changed
  what.
