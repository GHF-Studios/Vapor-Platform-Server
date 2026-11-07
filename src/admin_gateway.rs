use axum::body::{to_bytes, Body};
use axum::extract::Request;
use axum::http::header::{CONNECTION, CONTENT_LENGTH, HOST, TRANSFER_ENCODING};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use std::sync::OnceLock;
use std::time::Duration;

const MAX_ADMIN_REQUEST_BYTES: usize = 1024 * 1024;
const MAX_ADMIN_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
static HTTP_CLIENT: OnceLock<Result<reqwest::Client, reqwest::Error>> = OnceLock::new();

struct OwnerRoute {
    prefix: &'static str,
    origin: &'static str,
}

// This is a fixed list, not a general-purpose proxy. The owning service still
// checks the user's Identity session and capability before rendering/actions.
const OWNER_ROUTES: &[OwnerRoute] = &[
    OwnerRoute {
        prefix: "/admin/identity",
        origin: "http://127.0.0.1:7113",
    },
    OwnerRoute {
        prefix: "/admin/workshop",
        origin: "http://127.0.0.1:7115",
    },
    OwnerRoute {
        prefix: "/admin/artifacts",
        origin: "http://127.0.0.1:7116",
    },
    OwnerRoute {
        prefix: "/admin/build-cache",
        origin: "http://127.0.0.1:7117",
    },
];

pub async fn proxy_owner_admin(request: Request) -> Response {
    let (parts, body) = request.into_parts();
    if !matches!(
        parts.method.as_str(),
        "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE"
    ) {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let Some(owner) = OWNER_ROUTES
        .iter()
        .find(|route| path_is_in_section(&parts.uri.path().to_ascii_lowercase(), route.prefix))
    else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let mut url = format!("{}{}", owner.origin, parts.uri.path());
    if let Some(query) = parts.uri.query() {
        url.push('?');
        url.push_str(query);
    }

    let body = match to_bytes(body, MAX_ADMIN_REQUEST_BYTES).await {
        Ok(body) => body,
        Err(_) => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
    };

    let client = match HTTP_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
    }) {
        Ok(client) => client,
        Err(_) => return gateway_error(StatusCode::BAD_GATEWAY, "Admin gateway client unavailable."),
    };
    let mut outbound = client
        .request(parts.method.clone(), url)
        .timeout(Duration::from_secs(20));
    for (name, value) in &parts.headers {
        if is_hop_by_hop(name, &parts.headers) || name == HOST || name == CONTENT_LENGTH {
            continue;
        }
        outbound = outbound.header(name.clone(), value.clone());
    }

    let upstream = match outbound.body(body).send().await {
        Ok(response) => response,
        Err(_) => return gateway_error(StatusCode::BAD_GATEWAY, "Admin service unavailable."),
    };
    let status = upstream.status();
    let expected_length = upstream.content_length();
    if expected_length.is_some_and(|length| length > MAX_ADMIN_RESPONSE_BYTES as u64) {
        return gateway_error(
            StatusCode::BAD_GATEWAY,
            "Admin service response exceeded the gateway limit.",
        );
    }
    let upstream_headers = upstream.headers().clone();
    let mut upstream = upstream;
    let initial_capacity = expected_length
        .and_then(|length| usize::try_from(length).ok())
        .unwrap_or(0);
    let mut bytes = Vec::with_capacity(initial_capacity);
    loop {
        let chunk = match upstream.chunk().await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => break,
            Err(_) => return gateway_error(StatusCode::BAD_GATEWAY, "Admin service response failed."),
        };
        if bytes.len().saturating_add(chunk.len()) > MAX_ADMIN_RESPONSE_BYTES {
            return gateway_error(
                StatusCode::BAD_GATEWAY,
                "Admin service response exceeded the gateway limit.",
            );
        }
        bytes.extend_from_slice(&chunk);
    }

    let mut response = Response::new(Body::from(bytes));
    *response.status_mut() = status;
    for (name, value) in &upstream_headers {
        if !is_hop_by_hop(name, &upstream_headers) && name != CONTENT_LENGTH {
            response.headers_mut().append(name.clone(), value.clone());
        }
    }
    response
}

fn path_is_in_section(path: &str, prefix: &str) -> bool {
    path == prefix || path.strip_prefix(prefix).is_some_and(|rest| rest.starts_with('/'))
}

fn is_hop_by_hop(name: &HeaderName, headers: &HeaderMap) -> bool {
    let nominated = headers
        .get_all(CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|token| name.as_str().eq_ignore_ascii_case(token.trim()));
    nominated
        || name == CONNECTION
        || name == TRANSFER_ENCODING
        || name == "keep-alive"
        || name == "proxy-authenticate"
        || name == "proxy-authorization"
        || name == "te"
        || name == "trailer"
        || name == "upgrade"
}

pub async fn health() -> impl IntoResponse {
    axum::Json(serde_json::json!({ "status": "ok", "mode": "services" }))
}

fn gateway_error(status: StatusCode, message: &'static str) -> Response {
    (status, [(axum::http::header::CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=utf-8"))], message)
        .into_response()
}
