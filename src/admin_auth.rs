use axum::http::header::COOKIE;
use axum::http::{HeaderMap, StatusCode};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
#[cfg(test)]
use std::time::Duration;

const SESSION_COOKIE: &str = "vapor_identity_session";
const MAX_IDENTITY_RESPONSE_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AuthFailure {
    Unauthorized,
    Forbidden,
    Unavailable,
}

impl AuthFailure {
    pub(crate) fn status(self) -> StatusCode {
        match self {
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        }
    }
}

#[derive(Serialize)]
struct AuthorizationRequest<'a> {
    audience: &'static str,
    session_token: &'a str,
    policy: AuthorizationPolicy,
    include_profile_id: bool,
}

#[derive(Serialize)]
struct AuthorizationPolicy {
    mode: &'static str,
    requirements: [AuthorizationRequirement; 1],
}

#[derive(Serialize)]
struct AuthorizationRequirement {
    kind: &'static str,
    role: &'static str,
}

#[derive(Deserialize)]
struct AuthorizationDecision {
    allowed: bool,
    profile_id: Option<String>,
}

#[cfg(test)]
async fn authorize_root(
    headers: &HeaderMap,
    service_token: Option<&str>,
    endpoint: &str,
) -> Result<(), AuthFailure> {
    session_token(headers).ok_or(AuthFailure::Unauthorized)?;
    let service_token = service_token
        .filter(|token| valid_service_token(token))
        .ok_or(AuthFailure::Unavailable)?;
    if !valid_loopback_endpoint(endpoint) {
        return Err(AuthFailure::Unavailable);
    }

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(1))
        .timeout(Duration::from_secs(3))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| AuthFailure::Unavailable)?;
    authorize_root_with_client(headers, Some(service_token), endpoint, &client).await
}

pub(crate) async fn authorize_root_with_client(
    headers: &HeaderMap,
    service_token: Option<&str>,
    endpoint: &str,
    client: &reqwest::Client,
) -> Result<(), AuthFailure> {
    authorize_root_profile_with_client(headers, service_token, endpoint, client)
        .await
        .map(|_| ())
}

pub(crate) async fn authorize_root_profile_with_client(
    headers: &HeaderMap,
    service_token: Option<&str>,
    endpoint: &str,
    client: &reqwest::Client,
) -> Result<String, AuthFailure> {
    let session_token = session_token(headers).ok_or(AuthFailure::Unauthorized)?;
    let service_token = service_token
        .filter(|token| valid_service_token(token))
        .ok_or(AuthFailure::Unavailable)?;
    if !valid_loopback_endpoint(endpoint) {
        return Err(AuthFailure::Unavailable);
    }

    let request = AuthorizationRequest {
        audience: "vapor-platform-admin",
        session_token,
        policy: AuthorizationPolicy {
            mode: "all",
            requirements: [AuthorizationRequirement {
                kind: "role",
                role: "root",
            }],
        },
        include_profile_id: true,
    };
    let response = client
        .post(endpoint)
        .bearer_auth(service_token)
        .json(&request)
        .send()
        .await
        .map_err(|_| AuthFailure::Unavailable)?;
    if response.status() != StatusCode::OK {
        return Err(AuthFailure::Unavailable);
    }

    let mut body = Vec::new();
    let mut response = response;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| AuthFailure::Unavailable)?
    {
        if body.len().saturating_add(chunk.len()) > MAX_IDENTITY_RESPONSE_BYTES {
            return Err(AuthFailure::Unavailable);
        }
        body.extend_from_slice(&chunk);
    }
    let decision: AuthorizationDecision =
        serde_json::from_slice(&body).map_err(|_| AuthFailure::Unavailable)?;
    if decision.allowed {
        decision.profile_id.ok_or(AuthFailure::Unavailable)
    } else {
        Err(AuthFailure::Forbidden)
    }
}

pub(crate) fn session_token(headers: &HeaderMap) -> Option<&str> {
    let cookie = headers.get(COOKIE)?.to_str().ok()?;
    cookie.split(';').find_map(|part| {
        let (name, value) = part.trim().split_once('=')?;
        let value = value.trim();
        (name == SESSION_COOKIE
            && (32..=256).contains(&value.len())
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'))
        .then_some(value)
    })
}

fn valid_service_token(token: &str) -> bool {
    token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(crate) fn valid_loopback_endpoint(endpoint: &str) -> bool {
    let Ok(url) = Url::parse(endpoint) else {
        return false;
    };
    let loopback = url
        .host_str()
        .and_then(|host| host.parse::<IpAddr>().ok())
        .is_some_and(|address| address.is_loopback());
    url.scheme() == "http"
        && loopback
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
}

#[cfg(test)]
mod tests {
    use super::{authorize_root, authorize_root_with_client, valid_loopback_endpoint, AuthFailure};
    use axum::extract::State;
    use axum::http::{HeaderMap, HeaderValue, StatusCode};
    use axum::routing::post;
    use axum::{Json, Router};
    use serde::{Deserialize, Serialize};
    use std::sync::Arc;
    use tokio::net::TcpListener;

    const SESSION: &str =
        "vapor-session-12345678-1234-4234-9234-123456789abc-12345678-1234-4234-9234-123456789abc";
    const SERVICE_TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[derive(Clone)]
    struct MockIdentity {
        allowed: bool,
        observed: Arc<std::sync::Mutex<bool>>,
    }

    #[derive(Deserialize)]
    struct RequestBody {
        audience: String,
        session_token: String,
        policy: RequestPolicy,
        include_profile_id: bool,
    }

    #[derive(Deserialize)]
    struct RequestPolicy {
        mode: String,
        requirements: Vec<RequestRequirement>,
    }

    #[derive(Deserialize)]
    struct RequestRequirement {
        kind: String,
        role: String,
    }

    #[derive(Serialize)]
    struct Decision {
        allowed: bool,
        expires_at_unix: i64,
        profile_id: Option<String>,
    }

    async fn check(
        State(state): State<MockIdentity>,
        headers: HeaderMap,
        Json(body): Json<RequestBody>,
    ) -> (StatusCode, Json<Decision>) {
        let valid = headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value == format!("Bearer {SERVICE_TOKEN}"))
            && body.audience == "vapor-platform-admin"
            && body.session_token == SESSION
            && body.policy.mode == "all"
            && body.policy.requirements.len() == 1
            && body.policy.requirements[0].kind == "role"
            && body.policy.requirements[0].role == "root"
            && body.include_profile_id;
        *state.observed.lock().unwrap() = valid;
        (
            StatusCode::OK,
            Json(Decision {
                allowed: state.allowed && valid,
                expires_at_unix: 0,
                profile_id: Some("test-root-profile".to_string()),
            }),
        )
    }

    async fn mock_identity(
        allowed: bool,
    ) -> (
        String,
        Arc<std::sync::Mutex<bool>>,
        tokio::task::JoinHandle<()>,
    ) {
        let observed = Arc::new(std::sync::Mutex::new(false));
        let app = Router::new()
            .route("/authorize", post(check))
            .with_state(MockIdentity {
                allowed,
                observed: observed.clone(),
            });
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{address}/authorize"), observed, task)
    }

    fn session_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "cookie",
            HeaderValue::from_str(&format!("vapor_identity_session={SESSION}")).unwrap(),
        );
        headers
    }

    #[tokio::test]
    async fn missing_cookie_denies_without_calling_identity() {
        let (endpoint, observed, task) = mock_identity(true).await;
        let result = authorize_root(&HeaderMap::new(), Some(SERVICE_TOKEN), &endpoint).await;
        assert_eq!(result, Err(AuthFailure::Unauthorized));
        assert!(!*observed.lock().unwrap());
        task.abort();
    }

    #[tokio::test]
    async fn malformed_cookie_denies_without_calling_identity() {
        let (endpoint, observed, task) = mock_identity(true).await;
        let mut headers = HeaderMap::new();
        headers.insert(
            "cookie",
            HeaderValue::from_static("vapor_identity_session=bad token"),
        );
        let result = authorize_root(&headers, Some(SERVICE_TOKEN), &endpoint).await;
        assert_eq!(result, Err(AuthFailure::Unauthorized));
        assert!(!*observed.lock().unwrap());
        task.abort();
    }

    #[tokio::test]
    async fn missing_service_token_fails_closed() {
        let result = authorize_root(&session_headers(), None, "http://127.0.0.1:1/authorize").await;
        assert_eq!(result, Err(AuthFailure::Unavailable));
    }

    #[tokio::test]
    async fn identity_denial_is_forbidden_and_request_is_audience_bound() {
        let (endpoint, observed, task) = mock_identity(false).await;
        let result = authorize_root(&session_headers(), Some(SERVICE_TOKEN), &endpoint).await;
        assert_eq!(result, Err(AuthFailure::Forbidden));
        assert!(*observed.lock().unwrap());
        task.abort();
    }

    #[tokio::test]
    async fn root_session_is_allowed_only_by_identity_decision() {
        let (endpoint, observed, task) = mock_identity(true).await;
        let result = authorize_root(&session_headers(), Some(SERVICE_TOKEN), &endpoint).await;
        assert_eq!(result, Ok(()));
        assert!(*observed.lock().unwrap());
        task.abort();
    }

    #[tokio::test]
    async fn identity_timeout_fails_closed() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        });
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(20))
            .build()
            .unwrap();
        let result = authorize_root_with_client(
            &session_headers(),
            Some(SERVICE_TOKEN),
            &format!("http://{address}/authorize"),
            &client,
        )
        .await;
        assert_eq!(result, Err(AuthFailure::Unavailable));
        task.abort();
    }

    #[test]
    fn authorization_urls_must_be_literal_loopback_http() {
        assert!(valid_loopback_endpoint(
            "http://127.0.0.1:7110/api/identity/v1/internal/authorization/check"
        ));
        assert!(!valid_loopback_endpoint("https://127.0.0.1/authorize"));
        assert!(!valid_loopback_endpoint("http://localhost/authorize"));
        assert!(!valid_loopback_endpoint("http://example.com/authorize"));
        assert!(!valid_loopback_endpoint("http://127.0.0.1/authorize?url=x"));
    }
}
