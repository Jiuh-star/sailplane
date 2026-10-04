"""Verify the machines list updates over SSE without a reload."""
import re, subprocess, time
from playwright.sync_api import sync_playwright

BASE = "http://127.0.0.1:8080/admin"
cfg = open("/tmp/sailplane-demo/config.yaml").read()
API_KEY = re.search(r'api_key:\s*"([^"]+)"', cfg).group(1)

with sync_playwright() as p:
    browser = p.chromium.launch()
    page = browser.new_context().new_page()
    page.goto(f"{BASE}/login")
    page.fill("#api-key", API_KEY)
    page.click("button[type=submit]")
    page.wait_for_url(f"{BASE}/machines")
    page.wait_for_timeout(2500)

    rows_before = page.locator("table tbody tr").count()
    print(f"rows before: {rows_before}")

    # Register a fourth node while the page is open.
    key = subprocess.run(
        ["podman", "exec", "headscale-demo", "headscale", "preauthkeys",
         "create", "-u", "3", "--expiration", "1h", "-o", "json"],
        capture_output=True, text=True, timeout=60,
    ).stdout
    auth_key = re.search(r'"key"\s*:\s*"([^"]+)"', key).group(1)

    import os
    os.makedirs("/tmp/sailplane-demo/tsstate-phone", exist_ok=True)
    subprocess.run(["podman", "rm", "-f", "ts-demo-phone"], capture_output=True, text=True)
    result = subprocess.run([
        "podman", "run", "-d", "--name", "ts-demo-phone", "--network=host",
        "-e", "HTTP_PROXY=", "-e", "HTTPS_PROXY=", "-e", "http_proxy=", "-e", "https_proxy=",
        "-e", f"TS_AUTHKEY={auth_key}",
        "-e", "TS_STATE_DIR=/var/lib/tailscale",
        "-e", "TS_USERSPACE=true",
        "-e", "TS_EXTRA_ARGS=--login-server=http://127.0.0.1:8085 --hostname=demo-phone --accept-dns=false",
        "-v", "/tmp/sailplane-demo/tsstate-phone:/var/lib/tailscale:Z",
        "docker.io/tailscale/tailscale:latest",
    ], capture_output=True, text=True, timeout=120)
    if result.returncode != 0:
        print("podman run failed:", result.stderr.strip()[:300])
        raise SystemExit(2)
    print("started demo-phone, waiting for the UI to react (no reload)…")

    # Poll the DOM only; never call reload().
    deadline = time.time() + 30
    rows_after = rows_before
    while time.time() < deadline:
        rows_after = page.locator("table tbody tr").count()
        if rows_after > rows_before:
            break
        time.sleep(1)

    print(f"rows after:  {rows_after}  (waited {time.time() - deadline + 30:.0f}s)")
    names = page.locator("table tbody tr td:first-child a").all_inner_texts()
    print("machines visible:", names)
    browser.close()
    raise SystemExit(0 if rows_after > rows_before else 1)
