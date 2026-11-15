//! Demonstration-only read API. DO NOT expose to the public Internet before implementing auth.
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::{Json, Router, routing::get};
use mailbox_contracts::BootstrapResponse;
use mailbox_core::demo::{DemoStore, load_bootstrap};
use std::net::SocketAddr;
use tower_http::cors::{AllowOrigin, CorsLayer};

/// Browser origins allowed to read the demo API during local development.
const DEFAULT_DEV_ORIGINS: &str = "http://127.0.0.1:8080,http://localhost:8080";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "mailbox_api=info".into()),
        )
        .init();
    let origins =
        std::env::var("MAILBOX_CORS_ORIGINS").unwrap_or_else(|_| DEFAULT_DEV_ORIGINS.into());
    let app = router(&origins);
    let address: SocketAddr = std::env::var("MAILBOX_BIND")
        .unwrap_or_else(|_| "127.0.0.1:3000".into())
        .parse()?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    tracing::info!(%address, "MailBox demo API (no authentication): local-only");
    axum::serve(listener, app).await?;
    Ok(())
}

/// Only loopback http origins are accepted: the demo API has no authentication, so it must
/// never be made readable from an arbitrary website. Anything else in the list is ignored.
fn allowed_origins(list: &str) -> Vec<HeaderValue> {
    list.split(',')
        .map(str::trim)
        .filter(|o| {
            let rest = o.strip_prefix("http://").unwrap_or("");
            let host = rest.split(':').next().unwrap_or("");
            matches!(host, "127.0.0.1" | "localhost") && !rest.contains('/')
        })
        .filter_map(|o| HeaderValue::from_str(o).ok())
        .collect()
}

fn router(cors_origins: &str) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(allowed_origins(cors_origins)))
        .allow_methods([Method::GET])
        .allow_headers([header::ACCEPT]);
    Router::new()
        .route(
            "/healthz",
            get(|| async { Json(serde_json::json!({"status":"ok", "mode":"demo"})) }),
        )
        .route("/api/v1/demo/bootstrap", get(bootstrap))
        .layer(cors)
}

async fn bootstrap() -> Result<Json<BootstrapResponse>, StatusCode> {
    // Errors carry no detail to the client; the demo store has no user data to leak either way.
    load_bootstrap(&DemoStore)
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn get_with_origin(origin: Option<&str>) -> axum::response::Response {
        let mut req = Request::builder().uri("/api/v1/demo/bootstrap");
        if let Some(o) = origin {
            req = req.header(header::ORIGIN, o);
        }
        router(DEFAULT_DEV_ORIGINS)
            .oneshot(req.body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn bootstrap_returns_demo_payload_without_secrets() {
        let res = get_with_origin(None).await;
        assert_eq!(res.status(), StatusCode::OK);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let data: BootstrapResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(data.mode, "demo");
        assert_eq!(data.messages.len(), 13);
        let text = String::from_utf8_lossy(&body).to_lowercase();
        for forbidden in ["password", "token", "secret"] {
            assert!(!text.contains(forbidden));
        }
    }

    #[tokio::test]
    async fn cors_allows_local_dev_origin_only() {
        let ok = get_with_origin(Some("http://127.0.0.1:8080")).await;
        assert_eq!(
            ok.headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .unwrap(),
            "http://127.0.0.1:8080"
        );
        let other = get_with_origin(Some("https://evil.example")).await;
        assert!(
            other
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .is_none()
        );
    }

    #[test]
    fn configured_origins_must_be_loopback_http() {
        let list = "http://localhost:5173, https://app.example.com, http://127.0.0.1:8080/x, *, http://10.0.0.5:8080";
        assert_eq!(
            allowed_origins(list),
            vec![HeaderValue::from_static("http://localhost:5173")]
        );
    }

    #[tokio::test]
    async fn only_get_is_routed() {
        let req = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/demo/bootstrap")
            .body(Body::empty())
            .unwrap();
        let res = router(DEFAULT_DEV_ORIGINS).oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
    }
}
