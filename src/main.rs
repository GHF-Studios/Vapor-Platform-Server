mod admin;
mod admin_auth;
mod admin_gateway;
mod deploy_request_schema;
mod deploy_requests;
mod emergency_console;
mod operation_history;
mod pagination;
mod platform_ops_client;
mod recovery;
mod recovery_status;
mod ssh_access;
mod ssh_key;
mod ssh_policy;
mod ssh_schema;

use axum::Router;
use std::env;
use tokio::net::TcpListener;

const DEFAULT_BIND: &str = "127.0.0.1:7110";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bind = env::var("VAPOR_PLATFORM_BIND").unwrap_or_else(|_| DEFAULT_BIND.to_string());
    deploy_requests::initialize().await?;
    ssh_access::initialize().await?;
    operation_history::initialize().await?;
    emergency_console::initialize().await?;

    let app = if env::var("VAPOR_SERVER_MODE").as_deref() == Ok("services") {
        // In independent-services mode Platform remains the shared operator
        // gateway and owns only its own operations. Service pages/actions are
        // proxied to their owning processes by a fixed route allowlist.
        Router::new()
            .merge(admin::router())
            .merge(ssh_access::router())
            .route("/healthz", axum::routing::get(admin_gateway::health))
            .fallback(admin_gateway::proxy_owner_admin)
            .layer(axum::middleware::from_fn(admin::shell_middleware))
    } else {
        let identity = vapor_identity_server::routers_from_env().await?;
        let artifacts = vapor_artifact_server::routers_from_env().await?;
        let registry = vapor_registry_server::routers_from_env().await?;
        let build_cache = vapor_build_cache_server::routers_from_env();
        Router::new()
            .merge(vapor_homepage_server::app_router())
            .nest("/docs", vapor_docs_server::app_router_from_env().await?)
            .nest("/api/identity", identity.api)
            .merge(admin::router())
            .merge(ssh_access::router())
            .merge(identity.web)
            .nest(
                "/api/diagnostics",
                vapor_diagnostics_server::app_router_from_env().await?,
            )
            .nest("/api/registry", registry.api)
            .merge(registry.admin)
            .merge(artifacts.admin)
            .merge(artifacts.api)
            .nest("/api/build-cache", build_cache.api)
            .merge(build_cache.admin)
            .layer(axum::middleware::from_fn(admin::shell_middleware))
    };

    let listener = TcpListener::bind(&bind).await?;
    eprintln!("vapor-platform-server listening on {bind}");
    axum::serve(listener, app).await?;
    Ok(())
}
