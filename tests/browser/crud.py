"""Exercise the mutation paths through the real UI."""

import re
import sys
import time

from playwright.sync_api import sync_playwright

BASE = "http://127.0.0.1:8080/admin"
cfg = open("/tmp/sailplane-demo/config.yaml").read()
API_KEY = re.search(r'api_key:\s*"([^"]+)"', cfg).group(1)

results: list[tuple[str, bool, str]] = []


def check(name: str, ok: bool, detail: str = "") -> None:
    results.append((name, ok, detail))
    print(f"{'PASS' if ok else 'FAIL'}  {name}{'  — ' + detail if detail else ''}")


def wait_quiet(page, timeout=8000):
    """Wait until no dialog is open and the dropdown has settled."""
    page.wait_for_timeout(300)
    try:
        page.wait_for_function(
            """() => document.querySelectorAll(
                '[role=dialog],[role=alertdialog],[data-state=open].fixed.inset-0'
            ).length === 0""",
            timeout=timeout,
        )
    except Exception:
        pass
    page.wait_for_timeout(500)


def run(page) -> None:
    # --- rename a machine ---
    # Anchor on the node's name, not on row order: renaming the first row and
    # then "the first row" again renames whichever node sorted first after the
    # change, which silently swapped two names.
    original = "demo-laptop"
    page.goto(f"{BASE}/machines", wait_until="domcontentloaded")
    page.wait_for_timeout(1500)

    def open_rename(name: str) -> None:
        row = page.locator("table tbody tr").filter(has_text=name).first
        row.locator("button[aria-label='Machine actions']").click()
        page.get_by_role("menuitem", name="Edit machine name").click()
        page.wait_for_timeout(400)

    open_rename(original)
    page.locator("#machine-name").fill("demo-renamed")
    page.get_by_role("button", name="Save").click()
    wait_quiet(page)
    check("rename machine", "demo-renamed" in page.content(), f"was {original}")

    open_rename("demo-renamed")
    page.locator("#machine-name").fill(original)
    page.get_by_role("button", name="Save").click()
    wait_quiet(page)
    check("rename machine back", original in page.content())

    # --- edit ACL tags on the tagged node ---
    page.goto(f"{BASE}/machines", wait_until="domcontentloaded")
    page.wait_for_timeout(1500)
    rows = page.locator("table tbody tr")
    for index in range(rows.count()):
        if "demo-server" in rows.nth(index).inner_text():
            rows.nth(index).locator("button[aria-label='Machine actions']").click()
            break
    page.get_by_role("menuitem", name="Edit ACL tags").click()
    page.wait_for_timeout(500)
    tag_input = page.locator("input[placeholder='tag:server']")
    tag_input.fill("tag:prod")
    page.get_by_role("button", name="Add", exact=True).click()
    page.wait_for_timeout(300)
    page.get_by_role("button", name="Save").click()
    wait_quiet(page)
    check("add ACL tag", "tag:prod" in page.content())

    # --- create a pre-auth key ---
    page.goto(f"{BASE}/settings/auth-keys", wait_until="domcontentloaded")
    page.wait_for_timeout(1500)
    page.get_by_role("button", name="Create key").click()
    page.wait_for_timeout(500)
    page.locator("#acl-tags").fill("tag:testtag")
    page.get_by_role("button", name="Create key").last.click()
    page.wait_for_timeout(2000)
    check("create pre-auth key", "Pre-auth key created" in page.content())
    page.get_by_role("button", name="Done").click()
    wait_quiet(page)

    # --- create a user ---
    page.goto(f"{BASE}/users", wait_until="domcontentloaded")
    page.wait_for_timeout(1500)
    page.get_by_role("button", name="Add user").click()
    page.wait_for_timeout(400)
    page.locator("#new-username").fill("dave")
    page.locator("#new-email").fill("dave@example.com")
    page.get_by_role("button", name="Create user").click()
    wait_quiet(page)
    check("create user", "dave" in page.content())

    # --- edit the ACL policy (structured: add a host alias) ---
    # The name has to be new each run: re-adding an existing host is not a
    # change, so the policy Save button stays disabled and the click times out.
    host = f"crud-{int(time.time())}"
    page.goto(f"{BASE}/acls", wait_until="domcontentloaded")
    page.wait_for_timeout(2000)
    page.get_by_role("button", name="Add host").click()
    page.wait_for_timeout(400)
    page.locator("#host-name").fill(host)
    page.locator("#host-value").fill("100.64.0.99")
    page.get_by_role("button", name="Save", exact=True).last.click()
    page.wait_for_timeout(600)
    page.get_by_role("button", name="Save", exact=True).first.click()
    wait_quiet(page, 12000)
    check("save ACL policy", host in page.content())

    # Put the policy back: the demo is meant to survive its own tests.
    row = page.locator(f"span.font-medium:text-is('{host}')").locator("..")
    row.get_by_role("button", name="Delete").click()
    page.wait_for_timeout(400)
    page.get_by_role("button", name="Save", exact=True).first.click()
    wait_quiet(page, 12000)
    check("remove the test host again", host not in page.content())

    # --- DNS: add a search domain ---
    page.goto(f"{BASE}/dns", wait_until="domcontentloaded")
    page.wait_for_timeout(1800)
    page.locator("input[placeholder='corp.example.com']").fill("demo.internal")
    page.get_by_role("button", name="Add", exact=True).first.click()
    page.wait_for_timeout(6000)
    check("add DNS search domain", "demo.internal" in page.content())


with sync_playwright() as p:
    browser = p.chromium.launch()
    ctx = browser.new_context()
    # This suite selects elements by their English labels; pin the locale so a
    # Chinese browser default does not break it.
    ctx.add_init_script("localStorage.setItem('sailplane:locale', 'en')")
    page = ctx.new_page()
    errors: list[str] = []
    page.on("pageerror", lambda exc: errors.append(str(exc)))

    page.goto(f"{BASE}/login")
    page.fill("#api-key", API_KEY)
    page.click("button[type=submit]")
    page.wait_for_url(f"{BASE}/machines")
    page.wait_for_timeout(1500)

    try:
        run(page)
    except Exception as exc:  # noqa: BLE001 - report and keep going
        state = page.evaluate(
            "() => [...document.querySelectorAll('[data-state=open]')]"
            ".map(e => e.tagName + '[' + (e.getAttribute('role') || '') + ']'"
            "+ (e.className.includes('bg-black') ? ':overlay' : ''))"
        )
        print("  STATE AT FAILURE:", state, "url:", page.url)
        check("run completed", False, f"{type(exc).__name__}: {str(exc).splitlines()[0]}")

    browser.close()

failed = [name for name, ok, _ in results if not ok]
print()
print(f"{len(results) - len(failed)}/{len(results)} checks passed")
if errors:
    print("page errors:")
    for err in errors[:10]:
        print("  -", err)
sys.exit(1 if failed or errors else 0)
