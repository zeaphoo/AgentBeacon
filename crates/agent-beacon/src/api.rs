//! HTTP API、SSE 与静态资源托管。

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use agent_beacon_core::{Config, Effect, Event};
use axum::extract::rejection::JsonRejection;
use axum::extract::State;
use axum::http::{header, HeaderValue, StatusCode, Uri};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use rust_embed::RustEmbed;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::broadcast;

use crate::state::{BeaconState, Snapshot};

#[derive(RustEmbed)]
#[folder = "../../web/dist"]
struct Assets;

pub fn router(state: Arc<BeaconState>) -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/events", post(report_event))
        .route("/api/v1/agents", get(agents))
        .route("/api/v1/sessions", delete(clear_sessions))
        .route("/api/v1/state", get(state_snapshot))
        .route("/api/v1/stream", get(stream))
        .route("/api/v1/devices", get(devices))
        .route("/api/v1/devices/scan", post(scan))
        .route("/api/v1/control", post(control))
        .route("/api/v1/control/auto", post(control_auto))
        .route("/api/v1/config", get(get_config).put(put_config))
        .fallback(static_handler)
        .with_state(state)
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

async fn health(State(state): State<Arc<BeaconState>>) -> Json<serde_json::Value> {
    Json(state.health_json())
}

async fn report_event(
    State(state): State<Arc<BeaconState>>,
    payload: Result<Json<Event>, JsonRejection>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let Json(event) = payload.map_err(|rejection| ApiError::bad_request(rejection.body_text()))?;
    let session = state
        .apply_event(event)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    Ok(Json(json!({
        "aggregate": state.snapshot().aggregate,
        "session": session,
    })))
}

async fn agents(State(state): State<Arc<BeaconState>>) -> Json<serde_json::Value> {
    let snapshot = state.snapshot();
    Json(json!({
        "aggregate": snapshot.aggregate,
        "sessions": snapshot.sessions,
    }))
}

async fn clear_sessions(State(state): State<Arc<BeaconState>>) -> Json<serde_json::Value> {
    state.clear_sessions();
    Json(json!({ "ok": true, "aggregate": state.snapshot().aggregate }))
}

async fn state_snapshot(State(state): State<Arc<BeaconState>>) -> Json<Snapshot> {
    Json(state.snapshot())
}

async fn stream(
    State(state): State<Arc<BeaconState>>,
) -> Sse<impl futures_core::Stream<Item = Result<SseEvent, Infallible>>> {
    let initial = state.snapshot();
    let mut updates = state.subscribe_events();
    let mut shutdown = state.subscribe_shutdown();
    let stream = async_stream::stream! {
        yield Ok(SseEvent::default()
            .event("snapshot")
            .json_data(initial)
            .expect("snapshot 必须可序列化"));

        let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
        heartbeat.tick().await;
        loop {
            if *shutdown.borrow() {
                break;
            }
            tokio::select! {
                _ = heartbeat.tick() => {
                    yield Ok(SseEvent::default().comment("ping"));
                }
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        tracing::info!("SSE 流因服务关停结束");
                        break;
                    }
                }
                result = updates.recv() => {
                    match result {
                        Ok(snapshot) => {
                            yield Ok(SseEvent::default()
                                .event("update")
                                .json_data(snapshot)
                                .expect("snapshot 必须可序列化"));
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => break,
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn devices(State(state): State<Arc<BeaconState>>) -> Json<serde_json::Value> {
    Json(json!(state.snapshot().devices))
}

async fn scan(State(state): State<Arc<BeaconState>>) -> Json<serde_json::Value> {
    state.request_scan();
    Json(json!({ "ok": true }))
}

#[derive(Debug, Deserialize)]
struct ControlRequest {
    effect: Effect,
    #[serde(default)]
    duration_secs: Option<u64>,
    #[serde(default)]
    device: Option<String>,
}

async fn control(
    State(state): State<Arc<BeaconState>>,
    payload: Result<Json<ControlRequest>, JsonRejection>,
) -> Result<Json<Snapshot>, ApiError> {
    let Json(request) =
        payload.map_err(|rejection| ApiError::bad_request(rejection.body_text()))?;
    let duration = request
        .duration_secs
        .unwrap_or_else(|| state.config().manual_override_default_secs);
    if !(1..=3600).contains(&duration) {
        return Err(ApiError::bad_request("duration_secs 必须在 1 到 3600 之间"));
    }
    let device = request
        .device
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    state.set_override(request.effect, duration, device);
    Ok(Json(state.snapshot()))
}

async fn control_auto(State(state): State<Arc<BeaconState>>) -> Json<Snapshot> {
    state.clear_override();
    Json(state.snapshot())
}

async fn get_config(State(state): State<Arc<BeaconState>>) -> Json<Config> {
    Json(state.config())
}

async fn put_config(
    State(state): State<Arc<BeaconState>>,
    payload: Result<Json<Config>, JsonRejection>,
) -> Result<Json<Config>, ApiError> {
    let Json(config) = payload.map_err(|rejection| ApiError::bad_request(rejection.body_text()))?;
    config
        .validate()
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    state
        .set_config(config.clone())
        .map_err(ApiError::internal)?;
    Ok(Json(config))
}

fn asset_response(path: &str, data: Vec<u8>) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let cache = if path == "index.html" {
        "no-cache"
    } else {
        "public, max-age=3600"
    };
    let mut response = Response::new(axum::body::Body::from(data));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(mime.as_ref())
            .unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    response
}

async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path.starts_with("api/") {
        return ApiError::not_found("接口不存在").into_response();
    }
    let requested = if path.is_empty() { "index.html" } else { path };
    if let Some(file) = Assets::get(requested) {
        return asset_response(requested, file.data.into_owned());
    }
    match Assets::get("index.html") {
        Some(index) => asset_response("index.html", index.data.into_owned()),
        None => (
            StatusCode::OK,
            axum::response::Html(
                "<!doctype html><title>AgentBeacon</title><p>前端资源尚未构建，请运行 pnpm build:web。</p>",
            ),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use futures_util::StreamExt;
    use http_body_util::BodyExt;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    use tower::ServiceExt;

    fn test_state() -> Arc<BeaconState> {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            + u128::from(COUNTER.fetch_add(1, Ordering::Relaxed));
        let dir = std::env::temp_dir().join(format!("beacon-api-{unique}"));
        let store = crate::config_store::ConfigStore::new(dir.join("config.json"));
        let config = Config::default();
        store.save(&config).unwrap();
        BeaconState::new(config, store, 6980)
    }

    async fn body_json(response: Response) -> serde_json::Value {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn get(path: &str) -> Request<Body> {
        Request::builder()
            .method("GET")
            .uri(path)
            .body(Body::empty())
            .unwrap()
    }

    fn post(path: &str, body: &str) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(path)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    #[tokio::test]
    async fn health_and_event_flow() {
        let app = router(test_state());
        let response = app.clone().oneshot(get("/api/v1/health")).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let health = body_json(response).await;
        assert_eq!(health["status"], "ok");
        assert_eq!(health["port"], 6980);

        let response = app
            .clone()
            .oneshot(post(
                "/api/v1/events",
                r#"{"agent":"claude","state":"waiting"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let response = app.oneshot(get("/api/v1/agents")).await.unwrap();
        let agents = body_json(response).await;
        assert_eq!(agents["aggregate"], "waiting");
        assert_eq!(agents["sessions"][0]["session"], "claude:default");
    }

    #[tokio::test]
    async fn invalid_state_is_400() {
        let app = router(test_state());
        let response = app
            .oneshot(post(
                "/api/v1/events",
                r#"{"agent":"claude","state":"busy"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = body_json(response).await;
        assert!(body["error"].as_str().unwrap().contains("unknown variant"));
    }

    #[tokio::test]
    async fn invalid_config_is_rejected() {
        let app = router(test_state());
        let response = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/config")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"manual_override_default_secs":0}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn sse_sends_snapshot_and_updates() {
        let state = test_state();
        let app = router(state.clone());
        let response = app.oneshot(get("/api/v1/stream")).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let mut stream = response.into_body().into_data_stream();

        let first = tokio::time::timeout(Duration::from_secs(1), stream.next())
            .await
            .expect("等待首帧超时")
            .expect("流已结束")
            .unwrap();
        let first = String::from_utf8_lossy(&first);
        assert!(first.contains("event: snapshot"), "{first}");
        assert!(first.contains("\"aggregate\""), "{first}");

        let event = agent_beacon_core::Event {
            agent: "claude".into(),
            state: agent_beacon_core::AgentState::Error,
            session: None,
            message: None,
            ttl_secs: None,
        };
        state.apply_event(event).unwrap();

        let second = tokio::time::timeout(Duration::from_secs(1), stream.next())
            .await
            .expect("等待更新帧超时")
            .expect("流已结束")
            .unwrap();
        let second = String::from_utf8_lossy(&second);
        assert!(second.contains("event: update"), "{second}");
        assert!(second.contains("\"error\""), "{second}");
    }

    #[tokio::test]
    async fn sse_closes_when_shutdown_begins() {
        let state = test_state();
        let app = router(state.clone());
        let response = app.oneshot(get("/api/v1/stream")).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let mut stream = response.into_body().into_data_stream();
        let _first = tokio::time::timeout(Duration::from_secs(1), stream.next())
            .await
            .expect("等待快照超时")
            .expect("流已结束")
            .unwrap();

        state.begin_shutdown();
        let closed = tokio::time::timeout(Duration::from_secs(1), async {
            while stream.next().await.is_some() {}
        })
        .await;
        assert!(
            closed.is_ok(),
            "关停后 SSE 没有结束，graceful shutdown 会卡住"
        );
    }

    #[tokio::test]
    async fn static_files_fall_back_to_index() {
        let app = router(test_state());
        for path in ["/", "/settings"] {
            let response = app.clone().oneshot(get(path)).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[header::CONTENT_TYPE], "text/html");
        }
    }
}
