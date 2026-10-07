# Sailplane

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/logo-dark.svg">
  <img align="left" src="docs/assets/logo-light.svg" alt="Sailplane" width="128" height="128">
</picture>

Sailplane is a web UI for [Headscale](https://headscale.net). One binary serves
the single-page app, the JSON API and the live event stream. With every feature
enabled, the process stays under 50 MB RSS.

It is a from-scratch reimplementation of
[`tale/headplane`](https://github.com/tale/headplane) in Rust and Vue 3. The
configuration schema, the HTTP surface and the Headscale API contracts are the
same as the upstream project. The product names, environment variables and
default paths are different. Refer to the
[migration table](docs/CONFIGURATION.md#migrating-from-upstream-headplane).

<div align="center">
  <img src="docs/assets/screenshot.png" alt="The machines page">
  <em>The machines page</em>
</div>

## Features

- **Machines**: Search, filter and sort the list. Open a machine to see its
  routes, addresses and connectivity. Register, rename, expire or delete a
  machine. Edit routes and tags, or change the owner.
- **Users**: Manage Headscale users and Sailplane accounts in one place. Create,
  rename or delete an account, link the two, assign roles, or transfer
  ownership.
- **Access control**: Edit `acls`, `ssh`, `hosts`, `groups` and `tagOwners` in a
  structured editor. A HuJSON editor shows a side-by-side diff. Sailplane keeps
  unknown keys.
- **Access checker**: Ask if one machine can reach another machine on a port.
  The answer names the rule that decided it. Headscale enforces a policy.
  Sailplane explains the policy.
- **Topology**: See the policy as a graph, together with the route hubs and the
  relay regions of the tailnet.
- **DNS and DERP**: Edit the DNS settings and the relay map. Sailplane writes the
  changes into the Headscale config file.
- **Audit log**: See every request that changes state, with the actor and the
  result. The database keeps the most recent 5000 entries.
- **Auth**: Sign in with a Headscale API key, OpenID Connect (PKCE, discovery,
  role claims), or trusted reverse-proxy authentication.
- **Configuration in the UI**: Sailplane stores every setting in its database.
  Edit the settings from the Deployment page, or use the first-run onboarding
  wizard. Sailplane imports a legacy YAML file one time and then stops using it.
- **Live updates**: Server-sent events push machine and user changes to every
  open browser.
- **Browser SSH**: Open a terminal to a tailnet machine from the browser.
- **English and Simplified Chinese**: The UI follows the browser language.

## Installation

```sh
cd web && npm install && npm run build   # builds the SPA into web/dist
cargo build --release                    # embeds web/dist in the binary
./target/release/sailplane --config /etc/sailplane/config.yaml
```

See [docs/INSTALLATION.md](docs/INSTALLATION.md) for the full guide, the
reverse-proxy setup and a systemd unit. Container images are published to
`ghcr.io/jiuh-star/sailplane` for amd64 and arm64.

## Documentation

| Document | Contents |
| --- | --- |
| [INSTALLATION.md](docs/INSTALLATION.md) | Build, install, reverse proxy, TLS, containers |
| [CONFIGURATION.md](docs/CONFIGURATION.md) | Every configuration key, secrets, environment overrides, migration |
| [ARCHITECTURE.md](docs/ARCHITECTURE.md) | Design constraints, modules, the Go components, security position |
| [TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) | Known failure modes and fixes |
| [DEVELOPMENT.md](docs/DEVELOPMENT.md) | Build, test, demo, locales, comment style |
| [DEMO.md](docs/DEMO.md) | A self-contained demo with a throwaway Headscale |
| [UPSTREAM-CONTRACT.md](docs/UPSTREAM-CONTRACT.md) | The upstream behavior this project targets |

## Getting help

Open an issue in the project's issue tracker. Include the startup log and the
output of `sailplane --show-config` with the secrets redacted.

## Contributing

Read [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) first. Keep `cargo test` and
`cargo clippy --all-targets` green, follow the comment style, and change the
coupled locale keys in both languages.

## License

MIT

## AI Content Disclosure

Anthropic's Claude Code, an AI coding agent, writes the code, comments and
documentation in this repository. The maintainer sets the requirements, reviews
changes and runs the tests, but AI output can contain mistakes. Review the code
before you deploy it. Report problems in the issue tracker.
