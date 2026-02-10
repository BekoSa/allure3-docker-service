use axum::{
    response::{IntoResponse, Redirect},
    routing::{get, post},
    Router,
};
use http::{header::HeaderName, Request};
use std::time::Duration;
use tower_http::{
    classify::ServerErrorsFailureClass,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer, RequestId},
    trace::{OnFailure, OnResponse, TraceLayer},
};
use tracing::{info_span, Span};

use crate::handlers::{api, ui};
use crate::state::AppState;

async fn root_redirect() -> impl IntoResponse {
    Redirect::temporary("/ui/")
}

#[derive(Clone)]
struct MyOnResponse;

impl<B> OnResponse<B> for MyOnResponse {
    fn on_response(self, response: &http::Response<B>, latency: Duration, span: &Span) {
        let rid = response
            .extensions()
            .get::<RequestId>()
            .and_then(|v| v.header_value().to_str().ok())
            .unwrap_or("-");

        tracing::info!(
            parent: span,
            request_id = %rid,
            status = %response.status().as_u16(),
            latency_ms = %latency.as_millis(),
        );
    }
}

#[derive(Clone)]
struct MyOnFailure;

impl OnFailure<ServerErrorsFailureClass> for MyOnFailure {
    fn on_failure(
        &mut self,
        failure: ServerErrorsFailureClass,
        latency: Duration,
        span: &Span,
    ) {
        tracing::warn!(
            parent: span,
            failure = %failure,
            latency_ms = %latency.as_millis(),
        );
    }
}


pub fn router(state: AppState) -> Router {
    let request_id_header = HeaderName::from_static("x-request-id");

    Router::new()
        // / -> /ui/
        .route("/", get(root_redirect))
        // API
        .route("/api/v1/projects", get(api::list_projects))
        .route("/api/v1/projects/{project}/runs", post(api::upload_run))
        // UI
        .route("/ui/", get(ui::ui_index))
        .route("/ui/{project}/", get(ui::ui_project_home))
        .route("/ui/{project}/latest/", get(ui::ui_latest))
        .route("/ui/{project}/runs/{run_id}/", get(ui::ui_run_index))
        .route("/ui/{project}/runs/{run_id}/{*tail}", get(ui::ui_run_files))
        // request id: генерим и прокидываем обратно в response header
        .layer(PropagateRequestIdLayer::new(request_id_header.clone()))
        .layer(SetRequestIdLayer::new(request_id_header.clone(), MakeRequestUuid))
        // access logs: гарантированный лог на каждый запрос
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(move |req: &Request<_>| {
                    info_span!(
                "http.request",
                method = %req.method(),
                uri = %req.uri(),
            )
                })
                .on_response(MyOnResponse)
                .on_failure(MyOnFailure),
        )
        .with_state(state)
}
