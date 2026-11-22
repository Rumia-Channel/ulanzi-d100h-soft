//! Embedded web assets, same pattern as narou.rs frontend.rs: files live in
//! assets/ at the crate root and are compiled in via include_str!.
use axum::{
    body::Body,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};

const INDEX_HTML: &str = include_str!("../../assets/index.html");
const APP_JS: &str = include_str!("../../assets/app.js");
const STYLE_CSS: &str = include_str!("../../assets/style.css");

pub fn index() -> Response {
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], INDEX_HTML).into_response()
}

pub fn asset(path: &str) -> Response {
    let (content, mime) = match path {
        "app.js" => (APP_JS, "text/javascript; charset=utf-8"),
        "style.css" => (STYLE_CSS, "text/css; charset=utf-8"),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    Response::builder()
        .header(header::CONTENT_TYPE, mime)
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(content))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}
