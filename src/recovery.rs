use axum::http::header::CACHE_CONTROL;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use std::time::Duration;

const MAX_HELPER_OUTPUT: usize = 4096;
const HELPER: &str = "/usr/local/libexec/vapor-platform-ops";

pub(crate) fn router() -> Router {
    Router::new().route("/api/admin/recovery/status", get(status))
}

async fn status(headers: axum::http::HeaderMap) -> Response {
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

    // This must complete before sudo is invoked; the web process itself remains unprivileged.
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

    let result = tokio::time::timeout(Duration::from_secs(4), run_helper()).await;
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

async fn run_helper() -> Result<crate::recovery_status::RecoveryStatus, ()> {
    use tokio::io::AsyncReadExt;

    let mut child = tokio::process::Command::new("/usr/bin/sudo")
        .args(["-n", HELPER, "recovery-status"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| ())?;
    let stdout = child.stdout.take().ok_or(())?;
    let mut bytes = Vec::with_capacity(MAX_HELPER_OUTPUT);
    let mut limited = stdout.take((MAX_HELPER_OUTPUT + 1) as u64);
    tokio::time::timeout(Duration::from_secs(3), limited.read_to_end(&mut bytes))
        .await
        .map_err(|_| ())?
        .map_err(|_| ())?;
    if bytes.len() > MAX_HELPER_OUTPUT {
        return Err(());
    }
    let exit = tokio::time::timeout(Duration::from_secs(1), child.wait())
        .await
        .map_err(|_| ())?
        .map_err(|_| ())?;
    if !exit.success() {
        return Err(());
    }
    serde_json::from_slice(&bytes).map_err(|_| ())
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
