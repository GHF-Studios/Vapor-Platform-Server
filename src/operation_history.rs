use crate::pagination::{Page, PageCursor, PageQuery};
use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::header::CACHE_CONTROL;
use axum::http::{HeaderMap, HeaderValue, Request, StatusCode};
use axum::middleware::{from_fn, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone)]
struct OperationHistoryState {
    pool: SqlitePool,
    authz_endpoint: String,
    service_token: Option<String>,
    client: reqwest::Client,
}

#[derive(Serialize)]
struct OperationHistoryRecord {
    operation_id: String,
    operation: String,
    result: String,
    started_at_utc: String,
    finished_at_utc: String,
    started_at_unix: i64,
    finished_at_unix: i64,
    revision: String,
    server_mode: String,
}

pub(crate) fn router() -> Router {
    Router::new()
        .route("/api/admin/operations/history", get(list_history))
        .with_state(state())
        .route_layer(from_fn(no_store))
}

pub(crate) async fn initialize() -> Result<(), Box<dyn std::error::Error>> {
    let directory = state_directory();
    tokio::fs::create_dir_all(&directory).await?;
    let pool = create_pool();
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS platform_operation_history (
            operation_id TEXT PRIMARY KEY,
            operation TEXT NOT NULL CHECK(operation IN ('deploy','restore')),
            result TEXT NOT NULL CHECK(result IN ('success','failed')),
            started_at_unix INTEGER NOT NULL,
            finished_at_unix INTEGER NOT NULL CHECK(finished_at_unix >= started_at_unix),
            started_at_utc TEXT NOT NULL,
            finished_at_utc TEXT NOT NULL,
            revision TEXT NOT NULL,
            server_mode TEXT NOT NULL CHECK(server_mode = 'services')
        )",
    )
    .execute(&pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_platform_operation_history_finished ON platform_operation_history(finished_at_unix DESC, operation_id DESC)")
        .execute(&pool)
        .await?;
    Ok(())
}

fn state_directory() -> PathBuf {
    std::env::var_os("VAPOR_PLATFORM_STATE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/var/lib/vapor-server/platform"))
}

fn database_path() -> PathBuf {
    std::env::var_os("VAPOR_PLATFORM_DB")
        .map(PathBuf::from)
        .unwrap_or_else(|| state_directory().join("platform.sqlite3"))
}

fn create_pool() -> SqlitePool {
    SqlitePoolOptions::new()
        .max_connections(5)
        .connect_lazy_with(
            SqliteConnectOptions::new()
                .filename(database_path())
                .create_if_missing(true)
                .foreign_keys(true)
                .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal),
        )
}

fn state() -> OperationHistoryState {
    let endpoint = std::env::var("VAPOR_PLATFORM_IDENTITY_AUTHZ_URL").unwrap_or_else(|_| {
        "http://127.0.0.1:7110/api/identity/v1/internal/authorization/check".to_owned()
    });
    OperationHistoryState {
        pool: create_pool(),
        authz_endpoint: endpoint,
        service_token: std::env::var("VAPOR_PLATFORM_IDENTITY_AUTHZ_TOKEN").ok(),
        client: reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(1))
            .timeout(Duration::from_secs(3))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("valid operation-history authorization client"),
    }
}

async fn list_history(
    State(state): State<OperationHistoryState>,
    headers: HeaderMap,
    Query(query): Query<PageQuery>,
) -> Response {
    if let Err(failure) = crate::admin_auth::authorize_root_with_client(
        &headers,
        state.service_token.as_deref(),
        &state.authz_endpoint,
        &state.client,
    )
    .await
    {
        let error = match failure {
            crate::admin_auth::AuthFailure::Unauthorized => "sign_in_required",
            crate::admin_auth::AuthFailure::Forbidden => "root_required",
            crate::admin_auth::AuthFailure::Unavailable => "identity_authorization_unavailable",
        };
        return (failure.status(), Json(serde_json::json!({"error": error}))).into_response();
    }
    let cursor = match query.decoded_cursor() {
        Ok(cursor) => cursor,
        Err(()) => return (StatusCode::BAD_REQUEST, "invalid pagination cursor\n").into_response(),
    };
    let page_size = query.page_size();
    let rows = match fetch_rows(&state.pool, cursor.as_ref(), page_size).await {
        Ok(rows) => rows,
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let page = match Page::from_items(rows, page_size, |row| PageCursor {
        created_at_unix: row.get("finished_at_unix"),
        id: row.get("operation_id"),
    }) {
        Ok(page) => page,
        Err(()) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let items = page
        .items
        .into_iter()
        .map(|row| OperationHistoryRecord {
            operation_id: row.get("operation_id"),
            operation: row.get("operation"),
            result: row.get("result"),
            started_at_utc: row.get("started_at_utc"),
            finished_at_utc: row.get("finished_at_utc"),
            started_at_unix: row.get("started_at_unix"),
            finished_at_unix: row.get("finished_at_unix"),
            revision: row.get("revision"),
            server_mode: row.get("server_mode"),
        })
        .collect();
    Json(Page {
        items,
        next_cursor: page.next_cursor,
    })
    .into_response()
}

async fn fetch_rows(
    pool: &SqlitePool,
    cursor: Option<&PageCursor>,
    page_size: usize,
) -> Result<Vec<sqlx::sqlite::SqliteRow>, sqlx::Error> {
    let query = if cursor.is_some() {
        "SELECT operation_id, operation, result, started_at_unix, finished_at_unix,
                started_at_utc, finished_at_utc, revision, server_mode
         FROM platform_operation_history
         WHERE finished_at_unix < ? OR (finished_at_unix = ? AND operation_id < ?)
         ORDER BY finished_at_unix DESC, operation_id DESC LIMIT ?"
    } else {
        "SELECT operation_id, operation, result, started_at_unix, finished_at_unix,
                started_at_utc, finished_at_utc, revision, server_mode
         FROM platform_operation_history
         ORDER BY finished_at_unix DESC, operation_id DESC LIMIT ?"
    };
    let mut statement = sqlx::query(query);
    if let Some(cursor) = cursor {
        statement = statement
            .bind(cursor.created_at_unix)
            .bind(cursor.created_at_unix)
            .bind(&cursor.id);
    }
    statement.bind((page_size + 1) as i64).fetch_all(pool).await
}

async fn no_store(request: Request<Body>, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[cfg(test)]
mod tests {
    use super::{fetch_rows, list_history, OperationHistoryState};
    use crate::pagination::{Page, PageCursor, PageQuery};
    use axum::extract::{Query, State};
    use axum::http::{HeaderMap, StatusCode};
    use sqlx::sqlite::SqlitePoolOptions;
    use sqlx::Row;
    use std::time::Duration;

    #[tokio::test]
    async fn history_endpoint_requires_a_root_identity_session() {
        let state = OperationHistoryState {
            pool: SqlitePoolOptions::new()
                .max_connections(1)
                .connect_lazy("sqlite::memory:")
                .unwrap(),
            authz_endpoint: "http://127.0.0.1:7110/api/identity/v1/internal/authorization/check"
                .to_owned(),
            service_token: None,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(1))
                .build()
                .unwrap(),
        };

        let response =
            list_history(State(state), HeaderMap::new(), Query(PageQuery::default())).await;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn operation_history_pages_are_stable_when_finish_times_tie() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("CREATE TABLE platform_operation_history (operation_id TEXT, operation TEXT, result TEXT, started_at_unix INTEGER, finished_at_unix INTEGER, started_at_utc TEXT, finished_at_utc TEXT, revision TEXT, server_mode TEXT)")
            .execute(&pool)
            .await
            .unwrap();
        for id in ["a", "b", "c"] {
            sqlx::query("INSERT INTO platform_operation_history (operation_id, finished_at_unix) VALUES (?, 100)")
                .bind(id)
                .execute(&pool)
                .await
                .unwrap();
        }
        let first = Page::from_items(fetch_rows(&pool, None, 2).await.unwrap(), 2, |row| {
            PageCursor {
                created_at_unix: row.get("finished_at_unix"),
                id: row.get("operation_id"),
            }
        })
        .unwrap();
        assert_eq!(
            first
                .items
                .iter()
                .map(|row| row.get::<String, _>("operation_id"))
                .collect::<Vec<_>>(),
            vec!["c", "b"]
        );
        let cursor = PageCursor::decode(first.next_cursor.as_deref().unwrap()).unwrap();
        let second = Page::from_items(
            fetch_rows(&pool, Some(&cursor), 2).await.unwrap(),
            2,
            |row| PageCursor {
                created_at_unix: row.get("finished_at_unix"),
                id: row.get("operation_id"),
            },
        )
        .unwrap();
        assert_eq!(
            second
                .items
                .iter()
                .map(|row| row.get::<String, _>("operation_id"))
                .collect::<Vec<_>>(),
            vec!["a"]
        );
        assert!(second.next_cursor.is_none());
    }
}
