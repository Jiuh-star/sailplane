# Installation

Sailplane ships as one binary. The binary contains the web UI, the JSON API and
the server-sent event stream. Node.js is necessary only to build the frontend.
Rust is necessary only if you build from source.

## Requirements

- A Headscale server, version 0.27.0 or newer.
- Rust stable and Node.js 20 or newer for the build.
- A Linux host. Other Unix systems work. Windows is untested.

## Build from source

Build the frontend first. The Rust build embeds `web/dist` at compile time.

```sh
cd web && npm install && npm run build
cd .. && cargo build --release
```

The binary is `target/release/sailplane`.

## Install

```sh
install -Dm755 target/release/sailplane /usr/local/bin/sailplane
install -d /etc/sailplane /var/lib/sailplane
install -m600 config.yaml /etc/sailplane/config.yaml
```

The service user must have write access to `server.data_path`. When you enable
the DNS or restrictions pages, it must also have write access to the Headscale
config file.

## First run

Create an API key for Sailplane:

```sh
headscale apikeys create --expiration 90d
```

Put the key in `headscale.api_key`, then start the service:

```sh
sailplane --config /etc/sailplane/config.yaml
```

Open `<base_url><base_path>/` (default `/admin/`). The first account to sign in
becomes the owner.

Validate the configuration without starting:

```sh
sailplane --check
```

## systemd

```ini
[Unit]
Description=Sailplane
After=network-online.target

[Service]
ExecStart=/usr/local/bin/sailplane --config /etc/sailplane/config.yaml
Restart=on-failure
User=sailplane
Group=sailplane
ProtectSystem=strict
ReadWritePaths=/var/lib/sailplane
NoNewPrivileges=true

[Install]
WantedBy=multi-user.target
```

When Sailplane edits the Headscale config file, add `- /etc/headscale` to
`ReadWritePaths`.

## Reverse proxy

Forward the base path to Sailplane and everything else to Headscale. The default
base path is `/admin`.

Two endpoints are special:

- `GET <base_path>/events/live` is a server-sent event stream. Disable proxy
  buffering. Sailplane already sends `X-Accel-Buffering: no`.
- The terminal WebSocket under `<base_path>/ssh/` must have the HTTP upgrade headers.

nginx:

```nginx
location /admin/ {
    proxy_pass http://127.0.0.1:3000;
    proxy_set_header Host $host;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
    proxy_http_version 1.1;
    proxy_set_header Upgrade $http_upgrade;
    proxy_set_header Connection "upgrade";
    proxy_buffering off;
}

location / {
    proxy_pass http://127.0.0.1:8080;
    proxy_set_header Host $host;
}
```

Set `server.base_url` to the public URL. OIDC login must have it to build the
redirect URI.

## TLS

Terminate TLS at the proxy, or give Sailplane a certificate:

```yaml
server:
  tls_cert_path: /etc/sailplane/tls.crt
  tls_key_path: /etc/sailplane/tls.key
```

TLS forces `cookie_secure: true`.

## Health checks

`GET <base_path>/healthz` returns `{"status":"ok"}` and HTTP 200, or HTTP 500
when a dependency is down.

Set `SAILPLANE_LISTEN_FILE` to write the full health URL to a file after the
listener binds. Container health checks can read that file without parsing the
log.

## Containers

An image is published to GHCR for amd64 and arm64:

| Tag | Source |
| --- | --- |
| `X.Y.Z`, `X.Y`, `latest` | A `vX.Y.Z` git tag |
| `edge` | Every push to `main` |

```yaml
services:
  sailplane:
    image: ghcr.io/jiuh-star/sailplane:latest
    restart: unless-stopped
    ports:
      - "3000:3000"
    volumes:
      - ./config.yaml:/etc/sailplane/config.yaml:ro
      - sailplane-data:/var/lib/sailplane

volumes:
  sailplane-data:
```

The container runs as root because the optional sockets below and the Headscale
config file are root-owned by default. It listens on port 3000. It contains no
`tailscale` CLI. The agent reads the sidecar socket instead.

Mounts:

| Path | Purpose |
| --- | --- |
| `/etc/sailplane/config.yaml` | The configuration file. Required. |
| `/var/lib/sailplane` | The data directory. Use a volume, not a network file system: SQLite runs in WAL mode. |
| `/var/run/tailscale/tailscaled.sock` | The agent sidecar socket. Optional. |
| `/var/run/docker.sock` | The Docker or Podman socket for the reload integration. Optional. |
| The directory that holds `headscale.config_path` | Write access for the DNS and restrictions pages. Optional. |

`GET <base_path>/healthz` reports readiness: it returns 500 while Headscale is
unreachable. Validate a configuration file inside the image without starting
the server:

```sh
docker run --rm -v ./config.yaml:/etc/sailplane/config.yaml:ro \
  ghcr.io/jiuh-star/sailplane:latest --check
```

See [DEMO.md](DEMO.md) for a complete working example with the agent sidecar.

The first push creates the GHCR package as private, even for a public
repository. Set the package visibility to public before anonymous pulls work.
