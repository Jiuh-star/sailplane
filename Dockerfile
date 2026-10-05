# Build the frontend first. The Rust build embeds web/dist at compile time.

# --- frontend ---
FROM node:24-bookworm-slim AS web
WORKDIR /build/web
COPY web/package.json web/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY web/ ./
# A missing web/dist fails the Rust build. An empty web/dist does not: it
# embeds no assets and every page returns 503. Check the output here instead.
RUN npm run build && test -s dist/index.html

# --- backend ---
FROM rust:1.98-bookworm AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
# Keep the crate downloads in their own layer. Only a lockfile change repeats
# this step.
RUN cargo fetch --locked
# Compile the dependencies first, with a stub entry point. The stub does not
# reference the web module, so rust-embed expands only after web/dist exists.
RUN mkdir -p src \
 && printf 'fn main() {}\n' > src/main.rs \
 && cargo build --release --locked \
 && rm -rf src
COPY src ./src
COPY --from=web /build/web/dist ./web/dist
# COPY preserves the source mtimes, and they are older than the stub build.
# Cargo then treats the stub as current and skips the real build. Touch the
# sources to force it, and verify the result is not the stub.
RUN find src -name '*.rs' -exec touch {} + \
 && cargo build --release --locked \
 && test "$(/build/target/release/sailplane --version)" = "sailplane $(grep -m1 '^version = ' Cargo.toml | cut -d'"' -f2)"

# --- runtime ---
FROM debian:bookworm-slim AS runtime
ENV DEBIAN_FRONTEND=noninteractive
# ca-certificates serves the rustls native root store. tzdata serves the local
# time display.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates tzdata \
 && rm -rf /var/lib/apt/lists/*
COPY --from=build /build/target/release/sailplane /usr/local/bin/sailplane
RUN mkdir -p /etc/sailplane /var/lib/sailplane
ENV SAILPLANE_CONFIG_PATH=/etc/sailplane/config.yaml
EXPOSE 3000
ENTRYPOINT ["/usr/local/bin/sailplane"]

# No HEALTHCHECK instruction: the image has no HTTP client, and /admin/healthz
# reports Headscale reachability (readiness), not process liveness.
# The process runs as root on purpose: the optional docker and tailscaled
# sockets and the Headscale config file are root-owned by default.
