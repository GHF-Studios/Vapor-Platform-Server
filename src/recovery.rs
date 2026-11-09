use axum::http::header::CACHE_CONTROL;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use std::time::Duration;

pub(crate) fn router() -> Router {
    Router::new()
        .route("/api/admin/recovery/status", get(status))
        .route("/api/admin/recovery/snapshot", post(create_snapshot))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotRequest {
    confirmed: bool,
}

async fn status(headers: axum::http::HeaderMap) -> Response {
    let Some(session_token) = crate::admin_auth::session_token(&headers) else {
        return error(StatusCode::UNAUTHORIZED, "sign_in_required");
    };
    let endpoint = std::env::var("VAPOR_PLATFORM_IDENTITY_AUTHZ_URL").unwrap_or_else(|_| {
        "http://127.0.0.1:7110/api/identity/v1/internal/authorization/check".to_owned()
    });
    let service_token = std::env::var("VAPOR_PLATFORM_IDENTITY_AUTHZ_TOKEN").ok();
    let client = match reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(1))
        .timeout(Duration::from_secs(3))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client) => client,
        Err(_) => {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "identity_authorization_unavailable",
            )
        }
    };

    // This must complete before contacting the root-owned local operations socket.
    if let Err(failure) = crate::admin_auth::authorize_root_with_client(
        &headers,
        service_token.as_deref(),
        &endpoint,
        &client,
    )
    .await
    {
        return error(
            failure.status(),
            match failure {
                crate::admin_auth::AuthFailure::Unauthorized => "sign_in_required",
                crate::admin_auth::AuthFailure::Forbidden => "root_required",
                crate::admin_auth::AuthFailure::Unavailable => "identity_authorization_unavailable",
            },
        );
    }

    let result = tokio::time::timeout(Duration::from_secs(4), run_helper(session_token)).await;
    let status = match result {
        Ok(Ok(status)) if status.validate() => status,
        _ => {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "recovery_status_unavailable",
            )
        }
    };
    private_json(StatusCode::OK, status)
}

async fn create_snapshot(headers: HeaderMap, Json(request): Json<SnapshotRequest>) -> Response {
    if headers.get("origin").and_then(|value| value.to_str().ok())
        != Some(crate::admin_auth::browser_admin_origin().as_str())
        || !request.confirmed
    {
        return error(StatusCode::FORBIDDEN, "same_origin_confirmation_required");
    }
    let Some(session_token) = crate::admin_auth::session_token(&headers) else {
        return error(StatusCode::UNAUTHORIZED, "sign_in_required");
    };
    let endpoint = std::env::var("VAPOR_PLATFORM_IDENTITY_AUTHZ_URL").unwrap_or_else(|_| {
        "http://127.0.0.1:7110/api/identity/v1/internal/authorization/check".to_owned()
    });
    let service_token = std::env::var("VAPOR_PLATFORM_IDENTITY_AUTHZ_TOKEN").ok();
    let client = match reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(1))
        .timeout(Duration::from_secs(3))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client) => client,
        Err(_) => {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "identity_authorization_unavailable",
            )
        }
    };
    if let Err(failure) = crate::admin_auth::authorize_root_with_client(
        &headers,
        service_token.as_deref(),
        &endpoint,
        &client,
    )
    .await
    {
        return error(
            failure.status(),
            match failure {
                crate::admin_auth::AuthFailure::Unauthorized => "sign_in_required",
                crate::admin_auth::AuthFailure::Forbidden => "root_required",
                crate::admin_auth::AuthFailure::Unavailable => "identity_authorization_unavailable",
            },
        );
    }
    match tokio::time::timeout(Duration::from_secs(8), async {
        crate::platform_ops_client::request::<_, crate::platform_ops_client::OperationResult>(
            &serde_json::json!({
                "operation":"recovery_snapshot",
                "session_token":session_token
            }),
        )
        .await
    })
    .await
    {
        Ok(Ok(result)) if result.ok && result.state == "queued" => private_json(
            StatusCode::ACCEPTED,
            serde_json::json!({
                "ok": result.ok,
                "operation_id": result.operation_id,
                "state": result.state,
                "detail": result.detail
            }),
        ),
        Ok(Ok(result)) if result.state == "already_running" => private_json(
            StatusCode::CONFLICT,
            serde_json::json!({
                "ok": result.ok,
                "operation_id": result.operation_id,
                "state": result.state,
                "detail": result.detail
            }),
        ),
        _ => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "snapshot_request_unavailable",
        ),
    }
}

async fn run_helper(session_token: &str) -> Result<crate::recovery_status::RecoveryStatus, ()> {
    crate::platform_ops_client::request(&serde_json::json!({
        "operation":"recovery_status",
        "session_token":session_token
    }))
    .await
    .map_err(|_| ())
}

fn error(status: StatusCode, code: &'static str) -> Response {
    private_json(status, serde_json::json!({"error": code}))
}

fn private_json<T: serde::Serialize>(status: StatusCode, body: T) -> Response {
    let mut response = (status, Json(body)).into_response();
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    response
}
