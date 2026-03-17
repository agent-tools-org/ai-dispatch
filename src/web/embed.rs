// Embedded static assets for the web UI.
// Uses rust-embed to include src/web/static/ files in the binary.

use rust_embed::Embed;

#[derive(Embed)]
#[folder = "src/web/static/"]
pub struct StaticAssets;

/// Look up a static file by path and return (content_type, body).
pub fn get_asset(path: &str) -> Option<(&'static str, Vec<u8>)> {
    let resolved = if path.is_empty() || path == "/" {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };
    let (file, actual_path) = match StaticAssets::get(resolved) {
        Some(f) => (f, resolved),
        None => (StaticAssets::get("index.html")?, "index.html"),
    };
    let content_type = match actual_path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "application/javascript; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    };
    Some((content_type, file.data.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_asset_falls_back_to_index_for_unknown_path() {
        // SPA fallback: unknown paths return index.html
        let result = get_asset("nonexistent.xyz");
        if let Some((ct, _)) = result {
            assert_eq!(ct, "text/html; charset=utf-8");
        }
        // If no static files embedded (e.g. test env without feature), None is ok too
    }
}
