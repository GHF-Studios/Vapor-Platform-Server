mod admin;
mod admin_auth;
mod admin_gateway;
mod deploy_request_schema;
mod deploy_requests;
mod emergency_console;
mod github_oidc;
mod network_telemetry;
mod node_fleet;
mod operation_history;
mod pagination;
mod platform_ops_client;
mod process_snapshot;
mod recovery;
mod recovery_status;
mod server_fleet;
mod server_operation_schema;
mod server_operations;
mod service_telemetry;
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
    node_fleet::initialize().await?;
    server_operations::initialize().await?;
    server_fleet::initialize();

    // Platform is the shared admin gateway only. Product services own their
    // routes and run as separate processes behind the fixed proxy map.
    let app = Router::new()
        .merge(admin::router())
        .merge(ssh_access::router())
        .route("/healthz", axum::routing::get(admin_gateway::health))
        .fallback(admin_gateway::proxy_owner_admin)
        .layer(axum::middleware::from_fn(admin::shell_middleware));

    let listener = TcpListener::bind(&bind).await?;
    eprintln!("vapor-platform-server listening on {bind}");
    axum::serve(listener, app).await?;
    Ok(())
}
