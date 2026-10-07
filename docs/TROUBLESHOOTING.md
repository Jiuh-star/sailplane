# Troubleshooting

## The page shows "The Sailplane frontend has not been built"

The binary embeds `web/dist` at compile time. Build the frontend, then the
backend:

```sh
cd web && npm run build
cd .. && cargo build --release
```

For frontend work, set `SAILPLANE_STATIC_DIR=web/dist` to serve the files from
disk.

## Sign-in seems to succeed, but the browser stays on the login page

The session cookie has the `Secure` attribute and the connection is plain HTTP.
Browsers discard such cookies. Either serve HTTPS, or set
`server.cookie_secure: false` for a trusted local network.

## The DNS and restrictions pages are missing

`headscale.config_path` is unset, or the file is not readable. Sailplane also
must have write access to that file to save changes. Check the log. It prints
the detected access level at startup.

## The agent shows no client versions

The agent could not read the LocalAPI netmap. Check these points:

- `integration.agent.enabled` is `true`.
- The socket at `integration.agent.socket` exists and is mounted into the
  container.
- The daemon accepts debug requests. Run as root, or run
  `tailscale set --operator <user>` once.
- The agent node is in a tailnet policy rule that lets it see the target
  machines.

Without debug access, the agent falls back to `GET /localapi/v0/status` and
reports every field except the client version.

## Browser SSH fails to connect

- The error "The configured SSH proxy ... is not reachable" means
  `integration.ssh.proxy` points nowhere. For the sidecar setup, set
  `TS_SOCKS5_SERVER` so it exposes a SOCKS5 proxy.
- Check `integration.ssh.enabled`.
- The session runs under the server's tailnet identity. The operator must have
  write access to the machine. Read-only roles get no shell.
- For a plain sshd target, set `integration.ssh.private_key_path` or a password.

## The log says "headscale /version returned 404"

The Headscale server is older than 0.27.0. That version is necessary for the
API surface. Upgrade Headscale.

## A config edit reports a warning

Sailplane wrote the file, but Headscale did not reload. The API returns HTTP 200
with a `warning` field by design. Check the reload integration:

- `docker`: the container name or label matches, and the socket is reachable.
- `kubernetes` and `proc`: the `headscale serve` process is visible in `/proc`.

## OIDC login is unavailable

- The discovery document must be reachable from the Sailplane host.
- You must set `server.base_url`. Without it, the redirect URI falls back to the
  request host and can fail to match the provider registration.
- `oidc.disable_api_key_login: true` intentionally hides API-key login.

## The UI did not update after an upgrade

These changes are necessary for an upgrade from upstream Headplane:

| What | Old | New |
| --- | --- | --- |
| Environment variables | `HEADPLANE_*` | `SAILPLANE_*` |
| Config file | `/etc/headplane/config.yaml` | `/etc/sailplane/config.yaml` |
| Data directory | `/var/lib/headplane/` | `/var/lib/sailplane/` |
| Database file | `hp_persist.db` | `sailplane_persist.db` |
| Session cookie | `_hp_auth` | `_sailplane_auth` |

Sailplane ignores the old names. To keep the existing database, point
`server.data_path` at the old directory and rename the file. Users sign in
again after the cookie change.

## Reverse proxy problems

- The event stream stalls: disable proxy buffering for
  `<base_path>/events/live`.
- The terminal does not open: forward the `Upgrade` and `Connection` headers.
- Wrong links or redirects: set `server.base_url` to the public URL.

## Configuration errors at startup

The loader rejects unknown keys and explains the mismatch. Run
`sailplane --check` to validate without starting. `sailplane --show-config`
prints the resolved configuration, including secrets. Treat the output as
sensitive.
