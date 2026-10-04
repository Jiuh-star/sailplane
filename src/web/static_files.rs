//! Static asset serving for the SPA.
//!
//! Assets are embedded at compile time, so deployment is a single file.
//! `SAILPLANE_STATIC_DIR` overrides that with a directory on disk for the dev
//! server. The frontend build emits compressed `*.gz` and `*.br` variants,
//! which are served directly, so the server never compresses.

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

use super::state::SharedState;

#[derive(RustEmbed)]
#[folder = "web/dist"]
#[exclude = "*.map"]
struct Assets;

/// Directories whose contents are content-hashed and therefore immutable.
const IMMUTABLE_PREFIXES: [&str; 2] = ["assets/", "fonts/"];

/// Serves the SPA and falls back to `index.html` for client routes. `GET *`
pub async fn serve(
    State(state): State<SharedState>,
    uri: Uri,
    headers: axum::http::HeaderMap,
) -> Response {
    let path = uri.path();
    let prefix = state.prefix();

    // Inside a nested router axum supplies the path *without* the mount prefix,
    // but `Uri` still reports it verbatim when the fallback is reached from the
    // outer router. Accept both shapes rather than depending on which one wins.
    let without_prefix = match path.strip_prefix(prefix) {
        Some(rest) => rest,
        None => path,
    };
    let relative = without_prefix.trim_start_matches('/');

    // The dev static directory joins this onto a path on disk, so a `..`
    // segment would read outside it. Production is an embedded map lookup and
    // would 404 either way; reject both the same way.
    if relative
        .split('/')
        .any(|segment| segment == ".." || segment.contains('\0'))
    {
        return not_found();
    }

    // The bare prefix and the SPA root both render the shell.
    if relative.is_empty() {
        return serve_spa_index(&state);
    }

    // API paths must never fall back to HTML.
    if relative.starts_with("api/") || relative == "healthz" || relative == "events/live" {
        return not_found();
    }

    let encoding = preferred_encoding(headers.get(header::ACCEPT_ENCODING));
    if let Some(response) = lookup(relative, encoding) {
        return response;
    }

    // A request for a missing asset with an extension is a genuine 404; a path
    // without one is a client-side route and gets the shell.
    if relative.rsplit('/').next().is_some_and(|last| last.contains('.')) {
        return not_found();
    }

    serve_spa_index(&state)
}

fn serve_spa_index(state: &SharedState) -> Response {
    match read_asset("index.html") {
        Some(bytes) => html_response(inject_base_href(bytes, state.prefix())),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            "The Sailplane frontend has not been built. Run `npm run build` inside `web/`.",
        )
            .into_response(),
    }
}

/// Injects `<base href="…/">` so relative asset and API URLs resolve against
/// the configured mount point even on nested client routes.
fn inject_base_href(html: Vec<u8>, prefix: &str) -> Vec<u8> {
    if prefix.is_empty() {
        return html;
    }
    let Ok(text) = String::from_utf8(html.clone()) else {
        return html;
    };
    if text.contains("<base ") {
        return html;
    }

    let tag = format!("<base href=\"{prefix}/\">");
    let injected = match text.find("<head>") {
        Some(index) => {
            let split = index + "<head>".len();
            format!("{}{tag}{}", &text[..split], &text[split..])
        }
        None => format!("{tag}{text}"),
    };

    injected.into_bytes()
}

/// The compressed variant to serve, or `None` for the file itself.
type Encoding = Option<&'static str>;

fn lookup(relative: &str, encoding: Encoding) -> Option<Response> {
    // Directory-style requests resolve to their index.
    let candidates = if relative.ends_with('/') {
        vec![format!("{relative}index.html")]
    } else {
        vec![relative.to_string(), format!("{relative}/index.html")]
    };

    for candidate in candidates {
        // The build writes `.br`/`.gz` next to the original; a dev static
        // directory may only have the plain file, hence the fallback.
        let found = encoding
            .and_then(|suffix| {
                read_asset(&format!("{candidate}{suffix}")).map(|bytes| (bytes, Some(suffix)))
            })
            .or_else(|| read_asset(&candidate).map(|bytes| (bytes, None)));

        let Some((bytes, encoding)) = found else {
            continue;
        };

        let immutable = IMMUTABLE_PREFIXES
            .iter()
            .any(|prefix| candidate.starts_with(prefix));
        let content_type = mime_for(&candidate);
        return Some(asset_response(bytes, content_type, immutable, encoding));
    }
    None
}

/// The best precompressed variant the client accepts.
fn preferred_encoding(header: Option<&HeaderValue>) -> Encoding {
    let header = header.and_then(|value| value.to_str().ok()).unwrap_or("");
    if accepts(header, "br") {
        Some(".br")
    } else if accepts(header, "gzip") {
        Some(".gz")
    } else {
        None
    }
}

/// Whether `Accept-Encoding` allows `coding`, treating `q=0` as a refusal.
fn accepts(header: &str, coding: &str) -> bool {
    header.split(',').any(|entry| {
        let mut params = entry.split(';');
        let name = params.next().unwrap_or("").trim();
        if name != coding && name != "*" {
            return false;
        }
        !params.any(|param| {
            param
                .trim()
                .strip_prefix("q=")
                .and_then(|q| q.parse::<f32>().ok())
                .is_some_and(|q| q <= 0.0)
        })
    })
}

/// Reads an asset from disk (dev override) or the embedded bundle.
fn read_asset(path: &str) -> Option<Vec<u8>> {
    if let Ok(dir) = std::env::var("SAILPLANE_STATIC_DIR") {
        let full = std::path::Path::new(&dir).join(path);
        if full.is_file() {
            return std::fs::read(full).ok();
        }
        return None;
    }

    Assets::get(path).map(|file| file.data.into_owned())
}

fn asset_response(
    bytes: Vec<u8>,
    content_type: &'static str,
    immutable: bool,
    encoding: Encoding,
) -> Response {
    let cache_control = if immutable {
        "public, max-age=31536000, immutable"
    } else {
        "public, max-age=3600"
    };

    let mut headers = axum::http::HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(cache_control),
    );
    // The response depends on what the client said it can decode.
    headers.insert(header::VARY, HeaderValue::from_static("Accept-Encoding"));

    if let Some(suffix) = encoding {
        let coding = if suffix == ".br" { "br" } else { "gzip" };
        headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static(coding));
    }

    (StatusCode::OK, headers, Body::from(bytes)).into_response()
}

fn html_response(bytes: Vec<u8>) -> Response {
    (
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            ),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("no-cache"),
            ),
        ],
        Body::from(bytes),
    )
        .into_response()
}

fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "Not found").into_response()
}

/// Minimal extension-to-MIME mapping; avoids pulling in a mime database.
fn mime_for(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or_default() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "wasm" => "application/wasm",
        "txt" => "text/plain; charset=utf-8",
        "map" => "application/json",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_types_cover_the_bundle() {
        assert_eq!(mime_for("assets/index-abc.js"), "text/javascript; charset=utf-8");
        assert_eq!(mime_for("index.html"), "text/html; charset=utf-8");
        assert_eq!(mime_for("app.wasm"), "application/wasm");
        assert_eq!(mime_for("unknown.bin"), "application/octet-stream");
    }
}
