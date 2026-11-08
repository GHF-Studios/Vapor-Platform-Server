use sqlx::sqlite::SqliteConnection;
use sqlx::{Row, SqlitePool};

pub async fn ensure_audit_actor_columns(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    // Startup can overlap briefly during a service restart or rollback. Take
    // SQLite's write reservation before inspecting schema so two processes
    // cannot both observe a missing column/trigger and race the same DDL.
    let mut connection = pool.acquire().await?;
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *connection)
        .await?;

    let migration = async {
        ensure_column(
            &mut connection,
            "platform_ssh_access_requests",
            "decided_by_actor_kind",
            "TEXT NOT NULL DEFAULT 'identity_profile'",
        )
        .await?;
        ensure_column(
            &mut connection,
            "platform_ssh_access_requests",
            "decided_by_actor_label",
            "TEXT",
        )
        .await?;
        ensure_column(
            &mut connection,
            "platform_ssh_access_events",
            "actor_kind",
            "TEXT NOT NULL DEFAULT 'identity_profile'",
        )
        .await?;
        ensure_column(
            &mut connection,
            "platform_ssh_access_events",
            "actor_label",
            "TEXT",
        )
        .await?;

        sqlx::query("DROP TRIGGER IF EXISTS platform_ssh_access_insert_audit")
            .execute(&mut *connection)
            .await?;
        sqlx::query("DROP TRIGGER IF EXISTS platform_ssh_access_update_audit")
            .execute(&mut *connection)
            .await?;
        sqlx::query(
        "CREATE TRIGGER IF NOT EXISTS platform_ssh_access_insert_audit
         AFTER INSERT ON platform_ssh_access_requests
         BEGIN
           INSERT INTO platform_ssh_access_events(
             request_id, from_status, to_status, actor_profile_id, actor_kind, actor_label, at_unix
           ) VALUES (
             NEW.id, NULL, NEW.status, NEW.requester_profile_id, 'identity_profile', NULL, NEW.created_at_unix
           );
         END",
        )
        .execute(&mut *connection)
        .await?;
        sqlx::query(
            "CREATE TRIGGER IF NOT EXISTS platform_ssh_access_update_audit
         AFTER UPDATE OF status ON platform_ssh_access_requests
         WHEN OLD.status != NEW.status
         BEGIN
           INSERT INTO platform_ssh_access_events(
             request_id, from_status, to_status, actor_profile_id, actor_kind, actor_label, at_unix
           ) VALUES (
             NEW.id, OLD.status, NEW.status, NEW.decided_by_profile_id,
             NEW.decided_by_actor_kind, NEW.decided_by_actor_label, CAST(strftime('%s','now') AS INTEGER)
           );
         END",
        )
        .execute(&mut *connection)
        .await?;
        Ok::<(), sqlx::Error>(())
    }
    .await;

    match migration {
        Ok(()) => match sqlx::query("COMMIT").execute(&mut *connection).await {
            Ok(_) => Ok(()),
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                Err(error)
            }
        },
        Err(error) => {
            let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
            Err(error)
        }
    }
}

async fn ensure_column(
    connection: &mut SqliteConnection,
    table: &'static str,
    column: &'static str,
    definition: &'static str,
) -> Result<(), sqlx::Error> {
    let rows = sqlx::query(&format!("PRAGMA table_info({table})"))
        .fetch_all(&mut *connection)
        .await?;
    if rows.iter().any(|row| {
        row.try_get::<String, _>("name")
            .is_ok_and(|name| name == column)
    }) {
        return Ok(());
    }
    sqlx::query(&format!(
        "ALTER TABLE {table} ADD COLUMN {column} {definition}"
    ))
    .execute(&mut *connection)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    #[tokio::test]
    async fn migration_is_repeatable_and_audit_records_identity_and_local_actors() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE platform_ssh_access_requests (
                id TEXT PRIMARY KEY, requester_profile_id TEXT, decided_by_profile_id TEXT,
                status TEXT NOT NULL, created_at_unix INTEGER NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE platform_ssh_access_events (
                event_id INTEGER PRIMARY KEY AUTOINCREMENT, request_id TEXT NOT NULL,
                from_status TEXT, to_status TEXT NOT NULL, actor_profile_id TEXT, at_unix INTEGER NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        ensure_audit_actor_columns(&pool).await.unwrap();
        ensure_audit_actor_columns(&pool).await.unwrap();

        sqlx::query("INSERT INTO platform_ssh_access_requests (id, requester_profile_id, status, created_at_unix) VALUES ('identity', 'profile-1', 'pending', 10)")
            .execute(&pool).await.unwrap();
        sqlx::query("UPDATE platform_ssh_access_requests SET status='rejected', decided_by_profile_id='profile-2', decided_by_actor_kind='identity_profile' WHERE id='identity'")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO platform_ssh_access_requests (id, status, created_at_unix, decided_by_actor_kind, decided_by_actor_label) VALUES ('local', 'pending', 11, 'local_operator', 'local-root')")
            .execute(&pool).await.unwrap();
        sqlx::query("UPDATE platform_ssh_access_requests SET status='provisioning', decided_by_profile_id=NULL, decided_by_actor_kind='local_operator', decided_by_actor_label='local-root' WHERE id='local'")
            .execute(&pool).await.unwrap();

        let events = sqlx::query("SELECT request_id, actor_profile_id, actor_kind, actor_label FROM platform_ssh_access_events ORDER BY event_id")
            .fetch_all(&pool).await.unwrap();
        assert_eq!(events.len(), 4);
        assert_eq!(events[0].get::<String, _>("actor_kind"), "identity_profile");
        assert_eq!(events[0].get::<String, _>("actor_profile_id"), "profile-1");
        assert_eq!(events[1].get::<String, _>("actor_kind"), "identity_profile");
        assert_eq!(events[1].get::<String, _>("actor_profile_id"), "profile-2");
        assert_eq!(events[3].get::<String, _>("actor_kind"), "local_operator");
        assert_eq!(events[3].get::<String, _>("actor_label"), "local-root");
    }
}
