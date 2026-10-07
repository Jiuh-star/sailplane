# Upstream contract (Headplane 0.7.1)

Reference notes extracted from [`tale/headplane`](https://github.com/tale/headplane)
v0.7.1 (React Router 7 + Node). Sailplane targets this behavioral contract.

Lines marked **Sailplane:** record a deliberate deviation. See
[ARCHITECTURE.md](ARCHITECTURE.md) for the reasons.

## Headscale API

Base `<headscale.url>/api`. Auth: `Authorization: Bearer <api key>`.
Every request sends `Accept: application/json`. Upstream sends
`User-Agent: Headplane/<version>`. **Sailplane:** sends `Sailplane/<version>`.
Query params only on GET/DELETE. JSON body only on POST/PUT/PATCH.

Public (unauthenticated):

| Method | Path       | Notes                                   |
| ------ | ---------- | --------------------------------------- |
| GET    | `/version` | `{"version":"v0.28.0"}` — capability detection |
| GET    | `/health`  | 200 = healthy                           |

Authenticated:

| Method | Path                                   | Body / query                                                  |
| ------ | -------------------------------------- | ------------------------------------------------------------- |
| GET    | `/api/v1/node`                         | `{"nodes":[...]}`                                             |
| GET    | `/api/v1/node/{id}`                    | `{"node":{...}}`                                              |
| DELETE | `/api/v1/node/{id}`                    | —                                                             |
| POST   | `/api/v1/node/register?user=&key=`     | body `{user,key}` sent **twice** (query + JSON)               |
| POST   | `/api/v1/node/{id}/approve_routes`     | `{"routes":[...]}`                                            |
| POST   | `/api/v1/node/{id}/expire`             | `?disableExpiry=true|false` doubles as toggle-expiry (0.29+)  |
| POST   | `/api/v1/node/{id}/rename/{name}`      | name URL-encoded in path                                      |
| POST   | `/api/v1/node/{id}/tags`               | `{"tags":[...]}`                                              |
| POST   | `/api/v1/node/{id}/user`               | `{"user":"name"}` — only when Headscale < 0.28                |
| GET    | `/api/v1/user`                         | query `id?`/`name?`/`email?` (max one)                        |
| POST   | `/api/v1/user`                         | `{name,email?,displayName?,pictureUrl?}`                      |
| DELETE | `/api/v1/user/{id}`                    | —                                                             |
| POST   | `/api/v1/user/{id}/rename/{newName}`   | —                                                             |
| GET    | `/api/v1/preauthkey`                   | `?user=<id>`, unfiltered list on 0.28+ only                   |
| POST   | `/api/v1/preauthkey`                   | `{ephemeral,reusable,expiration,user?,aclTags?}`              |
| POST   | `/api/v1/preauthkey/expire`            | 0.28+: `{id}`. Pre-0.28: `{user:<numeric id>,key}`            |
| GET    | `/api/v1/apikey`                       | `{"apiKeys":[...]}`                                           |
| GET    | `/api/v1/policy`                       | `{"policy":"<huJSON>","updatedAt":string|null}`               |
| PUT    | `/api/v1/policy`                       | `{"policy":"<huJSON>"}`                                       |
| POST   | `/api/v1/auth/approve`                 | `{"authId":"..."}`                                            |

### Version capabilities

`/version` 404 ⇒ Headscale < 0.27.0 (keep retrying, do not boot-fail).
Unparseable versions (`dev`, Go pseudo-versions) ⇒ `unknown` ⇒ every capability true.

| Capability                       | ≥ version |
| -------------------------------- | --------- |
| `preAuthKeysHaveStableIds`       | 0.28.0    |
| `nodeTagsAreFlat`                | 0.28.0    |
| `nodeOwnerIsImmutable`           | 0.28.0    |
| `registerKeyIncludesAuthReqPrefix` | 0.29.0  |
| `keyExpiryCanBeDisabled`         | 0.29.0    |

Minimum supported: **0.27.0**.

### Version-specific quirks

- Register key: strip `hskey-authreq-` prefix below 0.29.0.
- Node tags: 0.28+ uses `tags`. Below that, it uses the union of `forcedTags` + `validTags`.
- Node owner change: unsupported on 0.28+ (`nodeOwnerIsImmutable`).
- Pre-auth key IDs only exist on 0.28+.

## Wire types

```
Machine: id, machineKey, nodeKey, discoKey, ipAddresses[], name, user?, lastSeen,
         expiry?, preAuthKey?, createdAt, registerMethod, tags[], givenName, online,
         approvedRoutes[], availableRoutes[], subnetRoutes[]
User:    id, name, createdAt, displayName?, email?, providerId?, provider?, profilePicUrl?
PreAuthKey: id, key, user?, reusable, ephemeral, used, expiration, createdAt, aclTags[]
Key:     id, prefix, expiration, createdAt, lastSeen
```

OIDC Headscale users carry `providerId` whose last path segment is the OIDC subject.

## Roles → capabilities (bitflags)

```
ui_access 1<<0                read_policy 1<<1             write_policy 1<<2
read_network 1<<3             write_network 1<<4           read_feature 1<<5
write_feature 1<<6            configure_iam 1<<7           read_machines 1<<8
write_machines 1<<9           read_users 1<<10             write_users 1<<11
generate_authkeys 1<<12       use_tags 1<<13               write_tailnet 1<<14
owner 1<<15                   generate_own_authkeys 1<<16
```

| Role            | Capabilities                                                       |
| --------------- | ------------------------------------------------------------------ |
| `owner`         | everything (131071)                                                |
| `admin`         | everything except `generate_own_authkeys`, `owner` (32767)          |
| `network_admin` | ui, r/w policy, r/w network, read_feature, read_machines, read_users, generate_authkeys, use_tags, write_tailnet |
| `it_admin`      | ui, read_policy, read_network, read_feature, write_feature, configure_iam, r/w machines, r/w users, generate_authkeys |
| `auditor`       | ui, read_policy, read_network, read_feature, read_machines, read_users, generate_own_authkeys |
| `viewer`        | ui, read_machines, read_users, generate_own_authkeys                |
| `member`        | none — no UI access                                                |

API-key sessions bypass all role checks (full access).
`canManageNode`: API-key user, or `write_machines`, or node owner == linked Headscale user.

## Persistence (SQLite at `<data_path>/hp_persist.db`)

**Sailplane:** uses `sailplane_persist.db` and does not read the old file.

- `host_info(host_id PK, payload JSON, updated_at)` — agent-reported HostInfo per node key.
- `users(id PK ulid, sub UNIQUE, name, email, picture, role, headscale_user_id UNIQUE,
   created_at, updated_at, last_login_at)`.
- `auth_sessions(id PK ulid, kind, user_id, api_key_hash, api_key_display, oidc_id_token,
   expires_at, created_at)`.

First user to log in becomes `owner`. Sailplane never assigns `owner` through a
role claim.

## Cookies

- `_hp_auth` — session. Value `base64url(json) + "." + base64url(HMAC-SHA256(payload, cookie_secret))`.
  Not encrypted: API-key sessions carry the raw key inside the payload.
  **Sailplane:** uses `_sailplane_auth`, with the same format.
- `__oidc_state` — httpOnly, 30 min, path `<base>/oidc/callback`, holds `{state,nonce,verifier,redirect_uri}`.
- `color_scheme` — `dark|light|system`, 34560000 s.

No CSRF tokens and no security headers upstream. Same-site Lax cookies are the
only protection. **Sailplane: adds CSRF protection and the usual security
headers** — see
[ARCHITECTURE.md](ARCHITECTURE.md#security-position).

## Server surface

- `GET /healthz` → `{status}` 200/500.
- `GET /api/info` → `server.info_secret` is necessary (403 when unset, 401/403 on bad bearer).
- `POST /api/color-scheme` → sets cookie, redirects to same-origin `returnTo`.
- `GET /events/live` → SSE. `hello` with resource versions, then `changed` events.
  `: heartbeat` every 30 s. The server polls nodes every 5 s and users every 15 s.
  **Sailplane:** sends a heartbeat every 15 s.
- `/login`, `/logout`, `/oidc/start`, `/oidc/callback`, `/ssh/:id`.
- SPA under `<base>` (default `/admin`).

## Policy mode detection

- `updatedAt === null` from GET ⇒ file mode ⇒ read-only.
- GET error containing `acl policy not found`, or 500 ⇒ empty policy, writable.
- PUT error containing `update is disabled` ⇒ 403 "Policy is not writable".
- PUT parse errors prefixed `parsing HuJSON:` / `parsing policy from bytes:` ⇒ 400 syntax error.

## Headscale config file edits

Sailplane parses YAML with comments and order preserved (upstream uses the
`yaml` Document API). Keys touched: `dns.magic_dns`, `dns.base_domain`,
`dns.nameservers.global`, `dns.nameservers.split.<name>` (null deletes),
`dns.search_domains`, `dns.override_local_dns`, `dns.extra_records`,
`oidc.allowed_domains/_groups/_users`.
DNS records can live in a separate **JSON array** file (`dns.extra_records_path`).
Sailplane rewrites that file with a 4-space indent. After any patch, the
configured integration reloads Headscale.

## Integrations

You can enable exactly one of `docker` / `kubernetes` / `proc` (`agent` is independent).

- **docker**: Engine API over `unix://` (or `tcp://`), API ≥ 1.24, target 1.44. Find container
  by name or label `me.tale.headplane.target=headscale`. Then `POST /containers/{id}/restart`.
  Then poll Headscale `/health` 10× at 1 s.
  **Sailplane:** looks for `sailplane.target=headscale` by default, or a container name.
- **kubernetes**: in-cluster service account. Optionally validate that the pod has
  `shareProcessNamespace`. Find the `headscale serve` PID in `/proc`, send SIGHUP, and poll
  health.
- **proc**: same /proc scan + SIGHUP.

## Agent / WebSSH

Upstream ships two Go components:

- **hp_agent** — a `tsnet` node that reports per-peer `HostInfo` (version, OS, endpoints,
  NetInfo, SSH host keys) that the Headscale API does not expose.
- **hp_ssh** — a Go/WASM userspace Tailscale node powering Browser SSH, so that the *browser*
  joins the tailnet.

**Sailplane:** replaces both with one rootless `tailscaled` sidecar, with no custom
binaries. The LocalAPI netmap gives the host info. A server-side SSH client reaches the
tailnet through the sidecar's SOCKS5 proxy. See
[ARCHITECTURE.md](ARCHITECTURE.md#the-go-components-replaced).

Two upstream constraints still apply:

- Debug access is necessary for the netmap endpoint (root, or `tailscale set --operator`).
  Without it, the client version is unavailable.
- Sailplane accepts host keys unverified. Tailscale SSH has none. WireGuard authenticates
  the peer. For an ordinary sshd target, the operator chose to trust the target.
