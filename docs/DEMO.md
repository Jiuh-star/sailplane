# Running the demo

The demo starts Sailplane against a throwaway Headscale with three real
Tailscale nodes. Every page has data.

## What it sets up

| Component | Where |
| --- | --- |
| Headscale 0.29.2 | podman container `headscale-demo`, API on `127.0.0.1:8085` |
| Podman REST API | `unix://$XDG_RUNTIME_DIR/podman/podman.sock` (for the Docker reload integration) |
| Sailplane | `./target/release/sailplane`, `http://0.0.0.0:8080/admin/` |
| Demo nodes | containers `ts-demo-laptop`, `ts-demo-desktop`, `ts-demo-server` (userspace Tailscale) |
| Agent sidecar | container `ts-agent`, a rootless `tailscaled` on the demo tailnet. Its socket is mounted at `/tmp/sailplane-demo/agent-sock/` so the agent can read the netmap, and it exposes a SOCKS5 proxy on `127.0.0.1:1086` for browser SSH. |

Sailplane state lives in `/tmp/sailplane-demo/`. The Headscale config it edits
lives in `/tmp/headscale-demo/config/config.yaml`.

## Start it

```sh
./scripts/demo.sh start
./scripts/demo.sh stop
./scripts/demo.sh reset     # wipe Headscale state, users and nodes, then restart
```

The script prints a freshly created login API key.

## Notes on the setup

- Headscale needs at least one DERP region. This host has an outbound proxy that
  the container cannot reach, so the demo mounts a placeholder DERP map at
  `/etc/headscale/derp.yaml`. The demo only uses the control-plane API, so no
  region is ever dialled.
- The demo nodes run `tailscale up --tun=userspace-networking`, which needs no
  `NET_ADMIN`.
- Headscale's `server_url` is `http://127.0.0.1:8085`. The node containers share
  the host network namespace, so they can reach it.
- `ts-demo-server` runs `tailscale up --ssh`, so it is a real Tailscale SSH
  target. The browser terminal opens a session into it through the sidecar's
  SOCKS5 proxy.
- `tailscale serve` needs root. The demo is therefore reachable at
  `http://<tailnet-ip>:8080/admin/`, not over an HTTPS `*.ts.net` name. For a
  friendlier URL, run `sudo tailscale serve --bg --http=80
  http://127.0.0.1:8080`.

## Verification

`tests/browser/` holds the browser checks:

- `screenshots.py` signs in, screenshots every page, and fails on any console
  error.
- `crud.py` exercises the mutation paths: rename a machine, edit ACL tags,
  create a pre-auth key, create a user, save the ACL policy, edit DNS.
- `live_updates.py` registers a new node while a page is open and asserts that
  the machines table updates over SSE without a reload.

Run them with the demo up:

```sh
uv run --with playwright python3 tests/browser/crud.py
```

Point `BASE` at the tailnet address (`http://<node>:8080/admin`) instead of
`127.0.0.1` when checking clipboard or origin behaviour. `127.0.0.1` is a secure
context and the tailnet address is not.

`crud.py` pins the UI locale to English, because it selects elements by their
labels.
