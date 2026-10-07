#!/usr/bin/env bash
#
# Runs a self-contained Sailplane demo: a throwaway Headscale in podman, three
# real Tailscale nodes registered against it, and Sailplane in front.
#
#   ./scripts/demo.sh start | stop | reset | status
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEMO_DIR="${DEMO_DIR:-/tmp/sailplane-demo}"
HS_DIR="${HS_DIR:-/tmp/headscale-demo}"
HS_CONTAINER="headscale-demo"
HS_PORT=8085
SP_PORT=8080
XSOCK="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/podman/podman.sock"

log() { printf '\033[1m%s\033[0m\n' "$*"; }

require() {
  command -v "$1" >/dev/null 2>&1 || { echo "missing required tool: $1" >&2; exit 1; }
}

# Containers must not inherit an ambient HTTP proxy: the demo is entirely local
# and the proxy would swallow the DERP map fetch and the API calls.
NO_PROXY_ENV=(-e HTTP_PROXY= -e HTTPS_PROXY= -e http_proxy= -e https_proxy= -e ALL_PROXY= -e all_proxy=)

start_podman_api() {
  mkdir -p "$(dirname "$XSOCK")"
  if [ ! -S "$XSOCK" ]; then
    log "starting the podman REST API on $XSOCK"
    nohup podman system service --time=0 "unix://$XSOCK" >"$DEMO_DIR/podman-api.log" 2>&1 &
    sleep 2
  fi
}

write_headscale_config() {
  mkdir -p "$HS_DIR/config" "$HS_DIR/data"
  cat > "$HS_DIR/config/derp.yaml" <<'YAML'
# Placeholder DERP map: the demo only exercises the control-plane API.
regions:
  1:
    regionid: 1
    regioncode: local
    regionname: Local
    nodes:
      - name: local
        regionid: 1
        hostname: 127.0.0.1
        derpport: 3478
        stunport: 3478
YAML
  cat > "$HS_DIR/config/config.yaml" <<YAML
server_url: http://127.0.0.1:$HS_PORT
listen_addr: 0.0.0.0:$HS_PORT
metrics_listen_addr: 0.0.0.0:9091
grpc_listen_addr: 127.0.0.1:50443
grpc_allow_insecure: false

private_key_path: /var/lib/headscale/private.key
noise:
  private_key_path: /var/lib/headscale/noise_private.key

prefixes:
  v4: 100.64.0.0/10
  v6: fd7a:115c:a1e0::/48
  allocation: sequential

derp:
  server:
    enabled: false
  urls: []
  paths:
    - /etc/headscale/derp.yaml
  auto_update_enabled: false

disable_check_updates: true

database:
  type: sqlite
  sqlite:
    path: /var/lib/headscale/db.sqlite

log:
  level: info
  format: text

policy:
  mode: database

dns:
  magic_dns: true
  base_domain: sailplane.demo
  override_local_dns: true
  nameservers:
    global:
      - 1.1.1.1
  search_domains:
    - sailplane.demo
YAML
  # Sailplane edits this file for the DNS and restrictions pages, so it has to
  # be writable by the user running the server.
  chmod 666 "$HS_DIR/config/config.yaml"
  chmod 644 "$HS_DIR/config/derp.yaml"
  chmod 777 "$HS_DIR/data"
}

start_headscale() {
  podman rm -f "$HS_CONTAINER" >/dev/null 2>&1 || true
  log "starting Headscale on 127.0.0.1:$HS_PORT"
  podman run -d --name "$HS_CONTAINER" \
    --label sailplane.target=headscale \
    "${NO_PROXY_ENV[@]}" \
    -p "127.0.0.1:$HS_PORT:$HS_PORT" \
    -v "$HS_DIR/config:/etc/headscale:Z" \
    -v "$HS_DIR/data:/var/lib/headscale:Z" \
    docker.io/headscale/headscale:0.29.2 serve >/dev/null
  for _ in $(seq 1 30); do
    curl -sf --noproxy '*' -o /dev/null "http://127.0.0.1:$HS_PORT/health" && return 0
    sleep 1
  done
  echo "Headscale did not become healthy; see: podman logs $HS_CONTAINER" >&2
  return 1
}

hs() { podman exec "$HS_CONTAINER" headscale "$@"; }

seed_users() {
  for user in alice bob carol; do
    hs users create "$user" >/dev/null 2>&1 || true
  done
}

seed_policy() {
  cat > "$DEMO_DIR/policy.hujson" <<'JSON'
// Demo policy for the Sailplane reimplementation.
{
  "acls": [
    { "action": "accept", "src": ["group:admins"], "dst": ["*:*"] },
    { "action": "accept", "src": ["group:engineering"], "dst": ["tag:server:22,80,443"] },
    { "action": "accept", "src": ["tag:server"], "dst": ["tag:server:*"] }
  ],
  "ssh": [
    { "action": "accept", "src": ["group:admins"], "dst": ["tag:server"], "users": ["root", "autogroup:nonroot"] },
    { "action": "check", "src": ["group:engineering"], "dst": ["tag:server"], "users": ["autogroup:nonroot"], "checkPeriod": "12h" }
  ],
  "hosts": { "git": "100.64.0.3", "metrics": "100.64.0.0/28" },
  "groups": { "group:admins": ["alice@"], "group:engineering": ["alice@", "bob@"] },
  "tagOwners": { "tag:server": ["group:admins"], "tag:prod": ["group:admins", "group:engineering"] },
  "autoApprovers": {
    "routes": { "10.0.0.0/8": ["tag:server"] },
    "exitNode": ["tag:prod"]
  }
}
JSON
  podman exec -i "$HS_CONTAINER" headscale policy set -f /dev/stdin < "$DEMO_DIR/policy.hujson" >/dev/null
}

start_node() {
  local name="$1" uid="$2" extra="${3:-}"
  local key
  key=$(hs preauthkeys create -u "$uid" --reusable --expiration 24h -o json \
        | python3 -c 'import json,sys; print(json.load(sys.stdin)["key"])')
  podman rm -f "ts-$name" >/dev/null 2>&1 || true
  mkdir -p "$DEMO_DIR/tsstate-$name"
  log "registering node $name"
  podman run -d --name "ts-$name" --network=host \
    "${NO_PROXY_ENV[@]}" \
    -e "TS_AUTHKEY=$key" \
    -e TS_STATE_DIR=/var/lib/tailscale \
    -e TS_USERSPACE=true \
    -e "TS_EXTRA_ARGS=--login-server=http://127.0.0.1:$HS_PORT --hostname=$name --accept-dns=false $extra" \
    -v "$DEMO_DIR/tsstate-$name:/var/lib/tailscale:Z" \
    docker.io/tailscale/tailscale:latest >/dev/null
}

# A rootless tailscaled that joins the tailnet and provides both halves of the
# Go-component replacement: its netmap feeds the host-info agent, and its
# SOCKS5 listener routes browser SSH into the tailnet.
seed_agent() {
  local key
  key=$(hs preauthkeys create -u 1 --reusable --expiration 24h --tags tag:agent -o json \
        | python3 -c 'import json,sys; print(json.load(sys.stdin)["key"])')
  rm -rf "$DEMO_DIR/agent-sock"; mkdir -p "$DEMO_DIR/agent-sock" "$DEMO_DIR/tsstate-agent"
  chmod 777 "$DEMO_DIR/agent-sock"
  podman rm -f ts-agent >/dev/null 2>&1 || true
  log "starting the agent sidecar"
  podman run -d --name ts-agent --network=host \
    "${NO_PROXY_ENV[@]}" \
    -e "TS_AUTHKEY=$key" \
    -e TS_STATE_DIR=/var/lib/tailscale \
    -e TS_USERSPACE=true \
    -e TS_SOCKET=/var/run/tailscale/tailscaled.sock \
    -e TS_SOCKS5_SERVER=127.0.0.1:1086 \
    -e "TS_EXTRA_ARGS=--login-server=http://127.0.0.1:$HS_PORT --hostname=sailplane-agent --accept-dns=false" \
    -v "$DEMO_DIR/tsstate-agent:/var/lib/tailscale:Z" \
    -v "$DEMO_DIR/agent-sock:/var/run/tailscale:Z" \
    docker.io/tailscale/tailscale:latest >/dev/null
}

seed_nodes() {
  start_node demo-laptop 1
  start_node demo-desktop 1
  # demo-server also advertises a subnet, so the topology page has a real
  # route to show (and the policy's autoApprovers approves it).
  start_node demo-server 2 "--ssh --advertise-routes=10.20.0.0/16"
  sleep 12
  local sid
  sid=$(hs nodes list -o json | python3 -c '
import json,sys
for node in json.load(sys.stdin):
    if node.get("given_name") == "demo-server":
        print(node["id"])
')
  [ -n "$sid" ] && hs nodes tag -i "$sid" -t tag:server >/dev/null || true
}

start_sailplane() {
  local ts_name api_key secret
  ts_name=$(tailscale status --json 2>/dev/null \
    | python3 -c 'import json,sys; print(json.load(sys.stdin)["Self"]["DNSName"].rstrip("."))' \
    || echo "sailplane-demo")
  api_key=$(hs apikeys create --expiration 30d | tail -1)
  secret=$(head -c 24 /dev/urandom | base64 | tr -d '/+=' | head -c 32)

  mkdir -p "$DEMO_DIR"
  umask 077
  cat > "$DEMO_DIR/config.yaml" <<YAML
server:
  host: 0.0.0.0
  port: $SP_PORT
  base_url: http://$ts_name:$SP_PORT
  data_path: $DEMO_DIR/
  cookie_secret: "$secret"
  cookie_secure: false

headscale:
  url: http://127.0.0.1:$HS_PORT
  public_url: http://127.0.0.1:$HS_PORT
  api_key: "$api_key"
  config_path: $HS_DIR/config/config.yaml

integration:
  docker:
    enabled: true
    socket: unix://$XSOCK
  # Deployment plumbing only: the sidecar in seed_agent exposes this socket and
  # its SOCKS5 listener. Whether the agent and browser SSH are switched on is a
  # web setting (Settings → Agent / SSH), so the demo leaves them off.
  agent:
    socket: $DEMO_DIR/agent-sock/tailscaled.sock
  ssh:
    proxy: 127.0.0.1:1086
    username: root
YAML
  chmod 600 "$DEMO_DIR/config.yaml"

  log "starting Sailplane on 0.0.0.0:$SP_PORT"
  "$ROOT/target/release/sailplane" --config "$DEMO_DIR/config.yaml" >"$DEMO_DIR/sailplane.log" 2>&1 &
  echo $! > "$DEMO_DIR/sailplane.pid"
  sleep 3

  # A stale process on the port makes the bind fail after the script has
  # already printed a URL. Check liveness first.
  if ! kill -0 "$(cat "$DEMO_DIR/sailplane.pid")" 2>/dev/null; then
    echo "Sailplane failed to start; last log lines:" >&2
    tail -20 "$DEMO_DIR/sailplane.log" >&2
    exit 1
  fi

  echo
  echo "  URL:     http://$ts_name:$SP_PORT/admin/"
  echo "  API key: $api_key"
  echo
}

stop_all() {
  [ -f "$DEMO_DIR/sailplane.pid" ] && kill "$(cat "$DEMO_DIR/sailplane.pid")" 2>/dev/null || true
  podman ps --format '{{.Names}}' | grep -E '^(ts-demo-|headscale-demo)' | xargs -r -n1 podman rm -f >/dev/null 2>&1 || true
  log "stopped"
}

seed_sailplane_build() {
  if [ ! -f "$ROOT/web/dist/index.html" ]; then
    log "building the frontend"
    (cd "$ROOT/web" && npm install && npm run build)
  fi
  # Always run cargo: the frontend is embedded in the binary, so a stale
  # frontend otherwise keeps being served.
  log "building the release binary"
  (cd "$ROOT" && cargo build --release)
}

case "${1:-start}" in
  start)
    require podman; require curl; require python3
    mkdir -p "$DEMO_DIR"
    start_podman_api
    write_headscale_config
    start_headscale
    seed_users
    seed_policy
    seed_agent
    seed_nodes
    seed_sailplane_build
    start_sailplane
    ;;
  reset)
    stop_all
    rm -rf "$HS_DIR/data" "$DEMO_DIR/tsstate-"* "$DEMO_DIR/sailplane_persist.db"*
    "$0" start
    ;;
  stop) stop_all ;;
  status)
    podman ps --format '{{.Names}}\t{{.Status}}' | grep -E '^(ts-demo-|headscale-demo)' || echo "not running"
    [ -f "$DEMO_DIR/sailplane.pid" ] && echo "sailplane pid $(cat "$DEMO_DIR/sailplane.pid")"
    ;;
  *) echo "usage: $0 {start|stop|reset|status}" >&2; exit 2 ;;
esac
