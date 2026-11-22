//! Embedded web assets, same pattern as narou.rs frontend.rs: files live in
//! assets/ at the crate root and are compiled in via include_str!.
use axum::{
    body::Body,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};

const INDEX_HTML: &str = include_str!("../../assets/index.html");
const STYLE_CSS: &str = include_str!("../../assets/style.css");

macro_rules! js {
    ($name:literal) => {
        (
            include_str!(concat!("../../assets/js/", $name)),
            "text/javascript; charset=utf-8"
        )
    };
}

pub fn index() -> Response {
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], INDEX_HTML).into_response()
}

pub fn asset(path: &str) -> Response {
    let (content, mime) = match path.strip_prefix("js/") {
        Some("dom.js") => js!("dom.js"),
        Some("dial.js") => js!("dial.js"),
        Some("feed.js") => js!("feed.js"),
        Some("device.js") => js!("device.js"),
        Some("profiles.js") => js!("profiles.js"),
        Some("main.js") => js!("main.js"),
        _ if path == "style.css" => (STYLE_CSS, "text/css; charset=utf-8"),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    Response::builder()
        .header(header::CONTENT_TYPE, mime)
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(content))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}
