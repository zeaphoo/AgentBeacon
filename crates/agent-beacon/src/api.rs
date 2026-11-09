//! HTTP API、SSE 与静态资源托管。

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use agent_beacon_core::{Config, Effect, Event};
use axum::extract::rejection::JsonRejection;
use axum::extract::{Query, State};
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
        .route("/api/v1/hooks", get(hooks).delete(clear_hooks))
        .route("/api/v1/agents", get(agents))
        .route("/api/v1/sessions", delete(clear_sessions))
        .route("/api/v1/state", get(state_snapshot))
        .route("/api/v1/stream", get(stream))
        .route("/api/v1/devices", get(devices))
        .route("/api/v1/devices/scan", post(scan))
        .route("/api/v1/control", post(control))
        .route("/api/v1/control/auto", post(control_auto))
        .route("/api/v1/config", get(get_config).put(put_config))
        .route("/api/v1/integrations", get(integrations_view))
        .route("/api/v1/integrations/settings", post(integrations_settings))
        .route(
            "/api/v1/integrations/install-all",
            post(integrations_install_all),
        )
        .route(
            "/api/v1/integrations/{target}/{action}",
            post(integrations_action),
        )
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
    let Json(event) = payload.map_err(|rejection| {
        let message = rejection.body_text();
        state.record_rejected_hook(&message);
        ApiError::bad_request(message)
    })?;
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

#[derive(Debug, Deserialize)]
struct HooksQuery {
    #[serde(default)]
    limit: Option<usize>,
}

async fn hooks(
    State(state): State<Arc<BeaconState>>,
    Query(query): Query<HooksQuery>,
) -> Json<serde_json::Value> {
    let limit = query.limit.unwrap_or(100);
    Json(json!({
        "capacity": state.journal().capacity(),
        "next_seq": state.journal().next_seq(),
        "entries": state.journal().entries(limit),
    }))
}

async fn clear_hooks(State(state): State<Arc<BeaconState>>) -> Json<serde_json::Value> {
    state.journal().clear();
    Json(json!({ "ok": true }))
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
                        tracing::info!("SSE stream closed during shutdown");
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
        return Err(ApiError::bad_request(
            "duration_secs must be between 1 and 3600",
        ));
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

/// `GET /api/v1/integrations`：可控台展示的全部 agent 集成与安装状态。
async fn integrations_view(State(state): State<Arc<BeaconState>>) -> Json<serde_json::Value> {
    Json(integrations_payload(&state))
}

fn integrations_payload(state: &BeaconState) -> serde_json::Value {
    let config = state.config();
    let entries = crate::integrations::list();
    let detected = entries.iter().filter(|entry| entry.detected).count();
    let installed = entries
        .iter()
        .filter(|entry| entry.installed_version.is_some())
        .count();
    let pending: Vec<&str> = entries
        .iter()
        .filter(|entry| entry.detected && entry.installed_version.is_none())
        .map(|entry| entry.id)
        .collect();
    json!({
        "auto_install": config.integrations.auto_install,
        "onboarded": config.integrations.onboarded,
        "detected": detected,
        "installed": installed,
        "pending": pending,
        "entries": entries,
    })
}

#[derive(Debug, Default, Deserialize)]
struct IntegrationsSettings {
    #[serde(default)]
    auto_install: Option<bool>,
    #[serde(default)]
    onboarded: Option<bool>,
}

/// `POST /api/v1/integrations/settings`：只改集成相关设置，不触碰其余配置。
async fn integrations_settings(
    State(state): State<Arc<BeaconState>>,
    payload: Result<Json<IntegrationsSettings>, JsonRejection>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let Json(settings) =
        payload.map_err(|rejection| ApiError::bad_request(rejection.body_text()))?;
    let mut config = state.config();
    if let Some(auto_install) = settings.auto_install {
        config.integrations.auto_install = auto_install;
    }
    if let Some(onboarded) = settings.onboarded {
        config.integrations.onboarded = onboarded;
    }
    state.set_config(config).map_err(ApiError::internal)?;
    Ok(Json(integrations_payload(&state)))
}

/// `POST /api/v1/integrations/install-all`：安装所有检测到但尚未安装的集成。
///
/// 顺带把首次启动提示标记为已处理，避免装完又弹一次。
async fn integrations_install_all(
    State(state): State<Arc<BeaconState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let results = crate::integrations::install_detected();
    let mut config = state.config();
    if !config.integrations.onboarded {
        config.integrations.onboarded = true;
        state.set_config(config).map_err(ApiError::internal)?;
    }
    Ok(Json(json!({
        "results": results
            .into_iter()
            .map(|(target, result)| match result {
                Ok(outcome) => json!({
                    "id": target.id(),
                    "outcome": outcome.id(),
                }),
                Err(error) => json!({
                    "id": target.id(),
                    "error": error,
                }),
            })
            .collect::<Vec<_>>(),
        "integrations": integrations_payload(&state),
    })))
}

/// `POST /api/v1/integrations/{target}/{install|upgrade|uninstall}`。
async fn integrations_action(
    State(state): State<Arc<BeaconState>>,
    axum::extract::Path((target, action)): axum::extract::Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let Some(target) = crate::integrations::Target::parse(target.trim()) else {
        return Err(ApiError::not_found(format!(
            "unknown integration {}; expected {}",
            target,
            crate::integrations::Target::ids()
        )));
    };
    let outcome = match action.as_str() {
        "install" => {
            crate::integrations::install(target, None, false).map(|(outcome, _)| outcome.id())
        }
        "upgrade" => crate::integrations::upgrade(target, None).map(|(outcome, _)| outcome.id()),
        "uninstall" => crate::integrations::uninstall(target, None).map(|_| "uninstalled"),
        other => {
            return Err(ApiError::not_found(format!(
                "unknown integration action {other}; expected install, upgrade, uninstall"
            )))
        }
    }
    .map_err(ApiError::bad_request)?;
    Ok(Json(json!({
        "id": target.id(),
        "action": action,
        "outcome": outcome,
        "integrations": integrations_payload(&state),
    })))
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
        return ApiError::not_found("endpoint not found").into_response();
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
                "<!doctype html><title>AgentBeacon</title><p>Frontend assets are not built. Run pnpm build:web.</p>",
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
    async fn hook_log_exposes_events_and_rejections() {
        let app = router(test_state());
        let response = app
            .clone()
            .oneshot(post(
                "/api/v1/events",
                r#"{"agent":"pi","state":"working","message":"bash"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let response = app
            .clone()
            .oneshot(post("/api/v1/events", r#"{"agent":"pi","state":"busy"}"#))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let response = app.oneshot(get("/api/v1/hooks?limit=10")).await.unwrap();
        let body = body_json(response).await;
        let entries = body["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["kind"], "event");
        assert_eq!(entries[0]["outcome"], "applied");
        assert_eq!(entries[0]["state"], "working");
        assert_eq!(entries[0]["aggregate_after"], "working");
        assert_eq!(entries[1]["outcome"], "rejected");
        assert!(entries[1]["error"]
            .as_str()
            .unwrap()
            .contains("unknown variant"));
    }

    #[tokio::test]
    async fn integrations_endpoint_lists_targets_and_settings() {
        let state = test_state();
        let app = router(state.clone());

        let response = app
            .clone()
            .oneshot(get("/api/v1/integrations"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_json(response).await;
        assert_eq!(body["auto_install"], false);
        assert_eq!(body["onboarded"], false);
        let entries = body["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 9);
        let codex = entries
            .iter()
            .find(|entry| entry["id"] == "codex")
            .expect("codex must be listed");
        assert_eq!(codex["kind"], "notify");
        assert_eq!(codex["bundled_version"], crate::VERSION);
        assert!(codex["state"].is_string());
        assert!(codex["detected"].is_boolean());

        // 设置接口只改集成开关，其余配置保持不变。
        let response = app
            .clone()
            .oneshot(post(
                "/api/v1/integrations/settings",
                r#"{"auto_install":true,"onboarded":true}"#,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_json(response).await;
        assert_eq!(body["auto_install"], true);
        assert_eq!(body["onboarded"], true);
        assert_eq!(state.config().server.port, 6980);
    }

    #[tokio::test]
    async fn integration_actions_report_unknown_targets() {
        let app = router(test_state());
        let response = app
            .clone()
            .oneshot(post("/api/v1/integrations/nope/install", "{}"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let response = app
            .oneshot(post("/api/v1/integrations/codex/explode", "{}"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
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
