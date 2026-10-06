# Configuration

Settings live in Sailplane's SQLite database and are edited from the **Deployment**
page in the UI. A YAML config file is still read for one-time migration, then
deprecated; see [Migrating a config file](#migrating-a-config-file).

The effective configuration is built in this order, later layers winning:

1. Built-in defaults.
2. Values stored in the database (the UI writes these).
3. `SAILPLANE_<SECTION>__<KEY>` environment overrides.

Only the data directory must be known before the database is opened, so it comes
from `SAILPLANE_DATA_PATH` (default `/var/lib/sailplane/`) or a legacy config
file's `server.data_path`.

## First-run onboarding

On a fresh deployment with no account, no single sign-on, and no Headscale API
key yet, the UI shows a setup wizard. It asks for the Headscale URL and API key,
then optionally the public URL, and creates the first account when you sign in.
Importing a config file (or setting the key in the environment) marks the
deployment as configured, so an upgrade never re-asks for values it already has.
The wizard is reachable without authentication, so it is guarded by one of:

- A loopback peer (open it from the host itself).
- A one-time setup token. The token is printed to the log at first start
  (`first-run setup token: …`) and written to `setup-token` under the data
  directory (mode `0600`, removed once onboarding completes). Send it in the
  `x-setup-token` header. Override it with `SAILPLANE_SETUP_TOKEN`. Repeated
  wrong tokens are refused until Sailplane restarts.

After onboarding, the same values are edited on the Deployment page. A change to
a field marked **restart required** takes effect on the next start.

## Settings API

Owner accounts can read and change settings over the API.

| Method | Path | Body |
| --- | --- | --- |
| GET | `/api/settings` | — |
| PUT | `/api/settings` | `{ "values": { "key": value } }`, `null` deletes |
| POST | `/api/settings/validate` | `{ "values": { … } }` |
| POST | `/api/settings/import` | `{ "yaml": "<YAML text>" }` |

Secret values are never returned; the response reports whether one is set.

## Migrating a config file

An existing YAML config file at `--config` / `SAILPLANE_CONFIG_PATH` (default
`/etc/sailplane/config.yaml`) is imported into the database on the first start
after an upgrade, then ignored. Sailplane logs a warning and continues. To import
explicitly, run `sailplane --import-config <path>`.

The keys below are the ones the file may contain. A typo in a key name stops the
import with an error; the loader rejects unknown keys.

---

The rest of this document describes the configuration schema. Every key is
stored in the database and edited from the UI; `*_path` keys still read a secret
from a file, which suits container secret mounts.

## Minimal example

```yaml
server:
  host: 0.0.0.0
  port: 3000
  base_url: https://sailplane.example.com
  data_path: /var/lib/sailplane/
  cookie_secret: "<exactly 32 characters>"

headscale:
  url: http://headscale:8080
  api_key: "<run: headscale apikeys create>"
  config_path: /etc/headscale/config.yaml   # enables the DNS and restrictions pages
```

## Environment overrides

Use `SAILPLANE_<SECTION>__<KEY>` to override one value. A double underscore
separates the section from the key. Single underscores stay in the key.

```sh
SAILPLANE_SERVER__PORT=8080 SAILPLANE_DEBUG=true sailplane
```

Values are parsed as YAML scalars. `true`, `3000` and `null` get their natural
types.

These variables do not follow the section pattern:

| Variable | Effect |
| --- | --- |
| `SAILPLANE_CONFIG_PATH` | Path to the config file. Same as `--config`. |
| `SAILPLANE_DEBUG_LOG` | Enables debug logging. Same as `debug: true`. |
| `SAILPLANE_LISTEN_FILE` | Writes the health URL to this file after binding. For container health checks. |
| `SAILPLANE_STATIC_DIR` | Serves the SPA from this directory instead of the embedded copy. Development only. |

`RUST_LOG` controls the log filter. The default filter is
`sailplane=info,warn`.

## Secrets

A secret can be inline or in a file:

```yaml
headscale:
  api_key_path: /run/secrets/headscale-api-key
```

Set the inline value or the `*_path` key, never both. Both produce a startup
error. The file may contain a trailing newline.

The keys with a `*_path` variant are `server.cookie_secret`,
`headscale.api_key` and `oidc.client_secret`. `integration.ssh.password` is
inline only.

## `server.*`

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `host` | string | `0.0.0.0` | Listen address. |
| `port` | integer | `3000` | Listen port. |
| `base_url` | string | none | Public URL, without the base path. Required for OIDC redirect URIs. |
| `base_path` | string | `/admin` | Path prefix for the UI and API. |
| `data_path` | path | `/var/lib/sailplane/` | Directory for the SQLite database and agent state. |
| `info_secret` | string | none | Bearer token for `GET /api/info`. The endpoint is disabled when unset. |
| `cookie_secret` | string | required | Exactly 32 characters. Signs the session cookies. |
| `cookie_secret_path` | path | none | File variant of `cookie_secret`. |
| `cookie_secure` | boolean | `true` | Sets the `Secure` attribute on cookies. Forced to `true` when TLS is enabled. |
| `cookie_domain` | string | none | `Domain` attribute of the cookies. |
| `cookie_max_age` | integer | `86400` | Session lifetime in seconds. Must be positive. |
| `tls_cert_path` | path | none | TLS certificate. Set it together with the key. |
| `tls_key_path` | path | none | TLS private key. |
| `proxy_auth` | table | none | Trusted reverse-proxy authentication. See below. |

### `server.proxy_auth.*`

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `enabled` | boolean | required | Enables header-based authentication. |
| `allowed_cidrs` | list | loopback | CIDRs whose headers are accepted. |
| `trusted_proxy_cidrs` | list | loopback | CIDRs allowed to set `ip_header`. |
| `ip_header` | string | none | Header with the client IP when another proxy sits in front. |
| `user_header` | string | `Remote-User` | Header with the user name. |
| `email_header` | string | none | Header with the email address. |
| `name_header` | string | none | Header with the display name. |
| `picture_header` | string | none | Header with the avatar URL. |

## `headscale.*`

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `url` | string | required | Internal Headscale API URL. Must start with `http://` or `https://`. |
| `public_url` | string | `url` | Public URL shown in the UI and in registration commands. |
| `api_key` | string | none | Headscale API key. Required for OIDC, proxy auth and the agent. |
| `api_key_path` | path | none | File variant of `api_key`. |
| `config_path` | path | none | Headscale config file. Enables the DNS and restrictions pages and config reloads. |
| `config_strict` | boolean | `true` | Deprecated upstream. Accepted and ignored. |
| `dns_records_path` | path | none | JSON file behind `dns.extra_records_path` in the Headscale config. |
| `tls_cert_path` | path | none | Pinned certificate for the Headscale API. |

## `integration.*`

Enable at most one reload method: `docker`, `kubernetes` or `proc`. The `agent`
and `ssh` blocks are independent.

### `integration.docker.*`

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `enabled` | boolean | required | Enables the Docker integration. |
| `container_name` | string | none | Name of the Headscale container. |
| `container_label` | string | `sailplane.target=headscale` | Label that identifies the Headscale container. |
| `socket` | string | `unix:///var/run/docker.sock` | Docker or Podman socket. |

### `integration.kubernetes.*`

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `enabled` | boolean | required | Enables the Kubernetes integration. |
| `pod_name` | string | none | Pod that runs Headscale. Defaults to the pod of this process. |
| `validate_manifest` | boolean | `true` | Refuses to reload when the pod does not share its process namespace. |

### `integration.proc.*`

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `enabled` | boolean | required | Finds `headscale serve` in `/proc` and sends `SIGHUP`. |

### `integration.agent.*`

The agent reports per-machine client information that the Headscale API does not
expose. See [ARCHITECTURE.md](ARCHITECTURE.md) for the data sources.

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `enabled` | boolean | required | Enables host info collection. |
| `host_name` | string | `sailplane-agent` | Name the agent uses on the tailnet. |
| `cache_ttl` | integer | `180000` | Cache lifetime in milliseconds. |
| `backend` | `system` or `external` | `system` | `system` reads the tailscaled LocalAPI. `external` drives an upstream `hp_agent` binary. |
| `socket` | path | `/var/run/tailscale/tailscaled.sock` | tailscaled socket for the `system` backend. |
| `executable_path` | path | `/usr/libexec/sailplane/agent` | Binary for the `external` backend. |
| `work_dir` | path | `/var/lib/sailplane/agent` | State directory for the `external` backend. |
| `tailscale_netns` | boolean | `true` | Passes `HEADPLANE_AGENT_TS_NETNS` to the external binary. |

### `integration.ssh.*`

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `enabled` | boolean | required | Enables browser SSH. |
| `port` | integer | `22` | TCP port on the target machine. |
| `username` | string | none | Default login name. The user can pick another one. |
| `proxy` | string | none | SOCKS5 proxy for tailnet access, usually the sidecar's `TS_SOCKS5_SERVER`. Connections go direct when unset. |
| `private_key_path` | path | none | Key for targets that use ordinary sshd authentication. |
| `password` | string | none | Password authentication. Prefer a key. |

## `oidc.*`

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `enabled` | boolean | `true` | Enables OpenID Connect login. |
| `issuer` | string | required | Issuer URL for discovery and `iss` validation. |
| `client_id` | string | required | OIDC client ID. |
| `client_secret` | string | required | OIDC client secret. |
| `client_secret_path` | path | none | File variant of `client_secret`. |
| `headscale_api_key` | string | none | Deprecated upstream fallback for `headscale.api_key`. |
| `authorization_endpoint` | string | discovery | Overrides the discovered endpoint. |
| `token_endpoint` | string | discovery | Overrides the discovered endpoint. |
| `userinfo_endpoint` | string | discovery | Overrides the discovered endpoint. |
| `jwks_endpoint` | string | discovery | Overrides the discovered endpoint. |
| `end_session_endpoint` | string | discovery | Overrides the discovered endpoint. |
| `use_end_session` | boolean | `false` | Uses RP-initiated logout. |
| `post_logout_redirect_uri` | string | `<base_url><base_path>/login?s=logout` | Redirect after logout. |
| `token_endpoint_auth_method` | `auto`, `client_secret_basic`, `client_secret_post`, `client_secret_jwt` | `auto` | Client authentication at the token endpoint. |
| `use_pkce` | boolean | `false` | Enables PKCE. |
| `disable_api_key_login` | boolean | `false` | Rejects API-key login. The API enforces this, not only the UI. |
| `scope` | string | `openid email profile` | Requested scopes. |
| `subject_claims` | list | empty | Claims tried in order for the subject. |
| `default_role` | string | `member` | Role for a new account. |
| `role_claim` | string | none | Claim that carries the role. |
| `allow_weak_rsa_keys` | boolean | `false` | Accepts RSA keys below the usual size. |
| `profile_picture_source` | `oidc` or `gravatar` | `oidc` | Source of the avatar. |
| `extra_params` | map | none | Extra parameters for the authorization request. |

## Validation

```sh
sailplane --check                  # validate and exit
sailplane --show-config            # print the resolved configuration
sailplane --show-config --check    # print, then validate
```

`--show-config` prints resolved secrets in clear text. Treat the output as
sensitive.

## Migrating from upstream Headplane

Sailplane is a clean break. It ignores the old names:

- `HEADPLANE_*` environment variables have no effect. Use `SAILPLANE_*`.
- The default config path changed to `/etc/sailplane/config.yaml`.
- The default data path changed to `/var/lib/sailplane/`.
- The database file changed to `sailplane_persist.db`. An old database is not
  read. Point `server.data_path` at the old directory and rename the file to
  keep the data.
- The session cookie changed to `_sailplane_auth`. Existing sessions end; users
  sign in again.

The configuration schema itself is the same. An existing Headplane config file
loads without changes, unless it relies on the old defaults.
