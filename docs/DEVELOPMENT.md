# Development

## Build and test

```sh
cargo test                     # unit tests
cargo clippy --all-targets     # must stay warning-free

cd web
npm run build                  # vue-tsc type check, then vite build
```

Build the frontend before the backend. `cargo build` embeds `web/dist` at
compile time, so a stale frontend stays inside the binary. When in doubt, run
`cargo clean -p sailplane` and build again.

## Frontend development

```sh
cd web
npm run dev
```

The Vite server listens on port 5173 and proxies `/api` to
`http://127.0.0.1:8080`. Point `SAILPLANE_STATIC_DIR` at `web/dist` to serve the
SPA from disk while you iterate.

## Demo environment

`scripts/demo.sh` starts a throwaway Headscale, three Tailscale nodes, a
`tailscaled` sidecar and Sailplane. See [DEMO.md](DEMO.md).

```sh
./scripts/demo.sh start
./scripts/demo.sh stop
./scripts/demo.sh reset    # wipe state and start fresh
```

The browser checks in `tests/browser/` drive the demo with Playwright:

```sh
uv run --with playwright python3 tests/browser/crud.py
uv run --with playwright python3 tests/browser/live_updates.py
uv run --with playwright python3 tests/browser/screenshots.py
```

Run browser tests against the tailnet address, not `127.0.0.1`, when they touch
clipboard or origin behaviour. The two differ in one way that matters:
`127.0.0.1` is a secure context and the tailnet address is not.

## Layout

```
src/                backend modules (see ARCHITECTURE.md)
web/
  src/views/        one component per page
  src/components/ui/  shadcn-vue primitives (reka-ui based)
  src/composables/  session, live stream, toasts, theme
  src/locales/      per-area message catalogues (en, zh-CN)
  public/           logo.svg and favicon.svg
docs/               this documentation
scripts/demo.sh     the demo environment
tests/browser/      Playwright checks
```

## Comment style

Comments follow Simplified Technical English: short sentences, active voice, one
term per concept, and no narrative. Delete a comment that restates the code.
Keep a comment that records a non-obvious fact: a protocol quirk, an external
system's behaviour, a security constraint.

Vocabulary: "machine" for the object in the UI and the API surface, "node" only
in Headscale wire types and netmap code, "tailnet" for the Tailscale network.

## Locales

The UI ships English and Simplified Chinese. Each area has one catalogue per
language under `web/src/locales/<lang>/`. Keep the keys in both languages
identical.

Three keys couple the frontend to the backend. Change them on both sides
together:

- `machine.statusTags.sailplaneAgent` matches the tag key from
  `src/web/presentation.rs`.
- `settings.deployment.sailplaneVersion` matches the field from
  `src/web/util.rs`.
- `nav.brand` is the visible product name.

A missing key renders as its dotted path, so check the UI in both languages
after a change.

## Logo assets

The mark is a nine-dot grid on a 24-unit box, in the idiom of Tailscale's mark.
Dot centres sit at `x, y ∈ {3, 12, 21}`. The wing row (`y = 12`) and the tail
(`12, 21`) are solid discs of radius 3; the nose (`12, 3`) is a hollow ring in
the brand color; the four corners are the same discs at 28% opacity, drawn as
thin rings so the grid stays visible without competing with the glider.

The same geometry lives in five files:

- `web/public/logo.svg` (adaptive, no plate)
- `web/public/favicon.svg` (adaptive, with plate)
- `web/src/components/shared/BrandMark.vue` (currentColor body, brand ring)
- `docs/assets/logo-light.svg` and `docs/assets/logo-dark.svg` (static, for the
  README)

Change one, change all five. The README floats the mark left of the title, so
the static files are read at 128 px. Check the favicon at 30 px and 16 px in
both color schemes before committing.

## Tooling notes

- No CI configuration exists yet. Run the commands above by hand.
- The repository has no remote yet; `docs/UPSTREAM-CONTRACT.md` records the
  upstream behaviour this project targets.
