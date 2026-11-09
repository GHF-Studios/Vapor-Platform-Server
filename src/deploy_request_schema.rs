use sqlx::{Row, SqlitePool};

pub(crate) async fn ensure_dispatch_columns(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    ensure_column(
        pool,
        "dispatch_state",
        "TEXT NOT NULL DEFAULT 'not_attempted'",
    )
    .await?;
    ensure_column(pool, "dispatch_attempts", "INTEGER NOT NULL DEFAULT 0").await?;
    ensure_column(pool, "dispatch_attempted_at_unix", "INTEGER").await?;
    ensure_column(pool, "dispatch_detail", "TEXT").await?;
    ensure_column(pool, "github_workflow_run_key", "TEXT").await?;
    sqlx::query("CREATE UNIQUE INDEX IF NOT EXISTS idx_platform_deploy_requests_github_run ON platform_deploy_requests(github_workflow_run_key) WHERE github_workflow_run_key IS NOT NULL")
        .execute(pool)
        .await?;
    Ok(())
}

async fn ensure_column(
    pool: &SqlitePool,
    column: &'static str,
    definition: &'static str,
) -> Result<(), sqlx::Error> {
    let rows = sqlx::query("PRAGMA table_info(platform_deploy_requests)")
        .fetch_all(pool)
        .await?;
    if rows.iter().any(|row| {
        row.try_get::<String, _>("name")
            .is_ok_and(|name| name == column)
    }) {
        return Ok(());
    }
    sqlx::query(&format!(
        "ALTER TABLE platform_deploy_requests ADD COLUMN {column} {definition}"
    ))
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::ensure_dispatch_columns;
    use sqlx::sqlite::SqlitePoolOptions;
    use sqlx::Row;

    async fn pre_dispatch_schema() -> sqlx::SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE platform_deploy_requests (
                id TEXT PRIMARY KEY,
                status TEXT NOT NULL,
                version INTEGER NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO platform_deploy_requests (id, status, version)
             VALUES ('request-1', 'pending', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    #[tokio::test]
    async fn adds_dispatch_columns_with_neutral_defaults_to_existing_requests() {
        let pool = pre_dispatch_schema().await;

        ensure_dispatch_columns(&pool).await.unwrap();

        let row = sqlx::query(
            "SELECT status, version, dispatch_state, dispatch_attempts,
                    dispatch_attempted_at_unix, dispatch_detail
             FROM platform_deploy_requests WHERE id = 'request-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.get::<String, _>("status"), "pending");
        assert_eq!(row.get::<i64, _>("version"), 1);
        assert_eq!(row.get::<String, _>("dispatch_state"), "not_attempted");
        assert_eq!(row.get::<i64, _>("dispatch_attempts"), 0);
        assert_eq!(
            row.get::<Option<i64>, _>("dispatch_attempted_at_unix"),
            None
        );
        assert_eq!(row.get::<Option<String>, _>("dispatch_detail"), None);
    }

    #[tokio::test]
    async fn dispatch_column_migration_is_repeatable() {
        let pool = pre_dispatch_schema().await;

        ensure_dispatch_columns(&pool).await.unwrap();
        ensure_dispatch_columns(&pool).await.unwrap();

        let columns = sqlx::query("PRAGMA table_info(platform_deploy_requests)")
            .fetch_all(&pool)
            .await
            .unwrap();
        for expected in [
            "dispatch_state",
            "dispatch_attempts",
            "dispatch_attempted_at_unix",
            "dispatch_detail",
            "github_workflow_run_key",
        ] {
            assert!(columns
                .iter()
                .any(|column| { column.get::<String, _>("name") == expected }));
        }
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM platform_deploy_requests WHERE id = 'request-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn one_github_workflow_attempt_cannot_create_multiple_tickets() {
        let pool = pre_dispatch_schema().await;
        ensure_dispatch_columns(&pool).await.unwrap();

        sqlx::query(
            "INSERT INTO platform_deploy_requests (id, status, version, github_workflow_run_key)
             VALUES ('request-2', 'pending', 1, '123:1')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let duplicate = sqlx::query(
            "INSERT INTO platform_deploy_requests (id, status, version, github_workflow_run_key)
             VALUES ('request-3', 'pending', 1, '123:1')",
        )
        .execute(&pool)
        .await;
        assert!(duplicate.is_err());
    }
}
