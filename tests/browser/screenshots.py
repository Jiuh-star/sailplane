"""Drive the Sailplane demo in a real browser and capture the main pages."""

import re
import sys

from playwright.sync_api import sync_playwright

BASE = "http://127.0.0.1:8080/admin"
OUT = "/tmp/sailplane-demo/shots"

# Read the API key straight out of the config; never printed.
_config = open("/tmp/sailplane-demo/config.yaml").read()
API_KEY = re.search(r'api_key:\s*"([^"]+)"', _config).group(1)


def main() -> int:
    with sync_playwright() as p:
        browser = p.chromium.launch()
        context = browser.new_context(viewport={"width": 1440, "height": 960})
        # Tabs are selected by their English labels; pin the locale so a
        # Chinese browser default does not change them.
        context.add_init_script("localStorage.setItem('sailplane:locale', 'en')")
        page = context.new_page()

        errors: list[str] = []
        page.on("pageerror", lambda exc: errors.append(f"pageerror: {exc}"))
        page.on(
            "console",
            lambda msg: errors.append(f"console.{msg.type}: {msg.text}")
            if msg.type == "error"
            else None,
        )

        # --- sign in ---
        page.goto(f"{BASE}/login", wait_until="networkidle")
        page.fill("#api-key", API_KEY)
        page.click("button[type=submit]")
        page.wait_for_url(f"{BASE}/machines", timeout=15000)
        page.wait_for_timeout(1200)
        page.screenshot(path=f"{OUT}/01-machines.png", full_page=True)
        print("captured machines")

        # --- machine detail ---
        page.click("table tbody tr:first-child a")
        page.wait_for_timeout(1200)
        page.screenshot(path=f"{OUT}/02-machine.png", full_page=True)
        print("captured machine detail")

        # --- users ---
        page.goto(f"{BASE}/users", wait_until="networkidle")
        page.wait_for_timeout(900)
        page.screenshot(path=f"{OUT}/03-users.png", full_page=True)
        print("captured users")

        # --- access control (structured + file tabs) ---
        page.goto(f"{BASE}/acls", wait_until="networkidle")
        page.wait_for_timeout(1200)
        page.screenshot(path=f"{OUT}/04-acls.png", full_page=True)
        page.get_by_role("tab", name="Tags & groups").click()
        page.wait_for_timeout(600)
        page.screenshot(path=f"{OUT}/05-acls-tags.png", full_page=True)
        print("captured acls")

        # --- dns ---
        page.goto(f"{BASE}/dns", wait_until="networkidle")
        page.wait_for_timeout(900)
        page.screenshot(path=f"{OUT}/06-dns.png", full_page=True)
        print("captured dns")

        # --- pre-auth keys ---
        page.goto(f"{BASE}/settings/auth-keys", wait_until="networkidle")
        page.wait_for_timeout(900)
        page.screenshot(path=f"{OUT}/07-auth-keys.png", full_page=True)
        print("captured auth keys")

        # --- settings ---
        page.goto(f"{BASE}/settings", wait_until="networkidle")
        page.wait_for_timeout(700)
        page.screenshot(path=f"{OUT}/08-settings.png", full_page=True)
        print("captured settings")

        # --- dark mode on the machines list ---
        page.goto(f"{BASE}/machines", wait_until="networkidle")
        page.evaluate("document.documentElement.classList.add('dark')")
        page.wait_for_timeout(600)
        page.screenshot(path=f"{OUT}/09-machines-dark.png", full_page=True)
        print("captured dark mode")

        browser.close()

        real_errors = [e for e in errors if "favicon" not in e.lower()]
        if real_errors:
            print("\nBROWSER ERRORS:")
            for err in real_errors[:20]:
                print(" -", err)
            return 1

        print("\nno browser errors")
        return 0


if __name__ == "__main__":
    sys.exit(main())
