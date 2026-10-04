# Installation

Sailplane ships as one binary. The binary contains the web UI, the JSON API and
the server-sent event stream. You need Node.js only to build the frontend, and
Rust only if you build from source.

## Requirements

- A Headscale server, version 0.27.0 or newer.
- Rust stable and Node.js 20 or newer for the build.
- A Linux host. Other Unix systems work; Windows is untested.

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

The service user needs write access to `server.data_path`. It also needs write
access to the Headscale config file when you enable the DNS or restrictions
pages.

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

Add `- /etc/headscale` to `ReadWritePaths` when Sailplane edits the Headscale
config file.

## Reverse proxy

Forward the base path to Sailplane and everything else to Headscale. The default
base path is `/admin`.

Two endpoints need special care:

- `GET <base_path>/events/live` is a server-sent event stream. Disable proxy
  buffering. Sailplane already sends `X-Accel-Buffering: no`.
- The terminal WebSocket under `<base_path>/ssh/` needs the HTTP upgrade headers.

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

Set `server.base_url` to the public URL. OIDC login needs it to build the
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

No image is published. Build the binary and put it in an image of your choice.
Mount the Headscale config file into the container when you want the DNS and
restrictions pages.

The container needs the tailscaled socket for the agent, or a sidecar with its
socket mounted. See [DEMO.md](DEMO.md) for a complete working example.
