// Bearer token auth middleware — protects all API routes except /health and /map/players.
// Token loaded from service.json config. Rejects with 401 if missing/wrong.

use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::Response,
};

pub async fn bearer_auth(req: Request, next: Next) -> Result<Response, StatusCode> {
    let cfg_token = req
        .extensions()
        .get::<String>()
        .cloned()
        .unwrap_or_default();

    let provided = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");

    if provided.is_empty() || provided != cfg_token {
        return Err(StatusCode::UNAUTHORIZED);
    }

    Ok(next.run(req).await)
}
