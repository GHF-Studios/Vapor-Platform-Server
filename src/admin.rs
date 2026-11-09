use axum::body::{to_bytes, Body};
use axum::extract::{Query, Request};
use axum::http::header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Json;
use axum::Router;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;
use tokio::io::AsyncReadExt;

const DEFAULT_PROBE_ORIGIN: &str = "http://127.0.0.1:7110";
const DEFAULT_IDENTITY_AUTHZ_URL: &str =
    "http://127.0.0.1:7110/api/identity/v1/internal/authorization/check";

const MAX_ADMIN_PAGE_BYTES: usize = 8 * 1024 * 1024;
const ADMIN_PAGE_FRAGMENT_CONTENT_TYPE: &str = "application/vnd.vapor.admin-page.v1+html";

struct AdminSection {
    group: &'static str,
    name: &'static str,
    path: Option<&'static str>,
    description: &'static str,
    service_ids: &'static [&'static str],
    state: SectionState,
}

#[derive(Clone, Copy)]
enum SectionState {
    Implemented,
    Planned,
}

// This is the only section inventory. Service health is reported separately by
// the overview endpoint; a mounted route must not imply that its service is up.
const MODULES: &[AdminSection] = &[
    AdminSection {
        group: "Operate",
        name: "Overview",
        path: Some("/admin"),
        description: "Service health and readiness",
        service_ids: &[
            "platform",
            "homepage",
            "docs",
            "identity",
            "diagnostics",
            "registry",
            "artifacts",
            "build-cache",
        ],
        state: SectionState::Implemented,
    },
    AdminSection {
        group: "Ecosystem",
        name: "Registry",
        path: Some("/admin/registry"),
        description: "Ecosystem source inventory; typed content catalog not implemented",
        service_ids: &["registry"],
        state: SectionState::Implemented,
    },
    AdminSection {
        group: "Accounts",
        name: "Identity",
        path: Some("/admin/identity"),
        description: "Accounts and access administration",
        service_ids: &["identity"],
        state: SectionState::Implemented,
    },
    AdminSection {
        group: "Ecosystem",
        name: "Artifacts",
        path: Some("/admin/artifacts"),
        description: "Signed Vapor releases",
        service_ids: &["artifacts"],
        state: SectionState::Implemented,
    },
    AdminSection {
        name: "Build Snapshots",
        group: "Ecosystem",
        path: Some("/admin/build-cache"),
        description: "Signed Cargo target snapshots; not a shared compiler cache",
        service_ids: &["build-cache"],
        state: SectionState::Implemented,
    },
    AdminSection {
        group: "Ecosystem",
        name: "Native builders",
        path: None,
        description: "VPS-managed Linux and Windows build orchestration is not available yet",
        service_ids: &[],
        state: SectionState::Planned,
    },
    AdminSection {
        group: "Operate",
        name: "Approvals",
        path: Some("/admin/approvals"),
        description: "Pending decisions from Platform and Identity",
        service_ids: &["platform", "identity"],
        state: SectionState::Implemented,
    },
    AdminSection {
        group: "Operate",
        name: "Deployments",
        path: Some("/admin/deployments"),
        description: "Deployment lifecycle and execution stages",
        service_ids: &["platform"],
        state: SectionState::Implemented,
    },
    AdminSection {
        group: "Operate",
        name: "SSH Access",
        path: Some("/admin/ssh-access"),
        description: "Temporary access grants and active sessions",
        service_ids: &["platform"],
        state: SectionState::Implemented,
    },
    AdminSection {
        group: "Operate",
        name: "Hosting",
        path: Some("/admin/hosting"),
        description: "Service health and feature readiness",
        service_ids: &[
            "platform",
            "homepage",
            "docs",
            "identity",
            "diagnostics",
            "registry",
            "artifacts",
            "build-cache",
        ],
        state: SectionState::Implemented,
    },
    AdminSection {
        group: "Infrastructure",
        name: "Servers",
        path: Some("/admin/servers"),
        description: "Machine inventory and private host telemetry",
        service_ids: &["platform"],
        state: SectionState::Implemented,
    },
    AdminSection {
        group: "Operate",
        name: "History",
        path: Some("/admin/history"),
        description: "Recent owner-sourced request and operation history",
        service_ids: &["platform", "identity"],
        state: SectionState::Implemented,
    },
    AdminSection {
        group: "Emergency",
        name: "Recovery",
        path: Some("/admin/recovery"),
        description: "Recovery evidence and backup status",
        service_ids: &["platform"],
        state: SectionState::Implemented,
    },
    AdminSection {
        group: "Emergency",
        name: "Browser console",
        path: Some("/admin/emergency-console"),
        description: "Explicit short-lived root browser terminal",
        service_ids: &[],
        state: SectionState::Implemented,
    },
];

const CSS: &str = r#"
:root { color-scheme: dark; font: 14px/1.45 system-ui, sans-serif; background:#111821; color:#e4eaf1; }
* { box-sizing:border-box; }
body { margin:0; min-height:100vh; }
.shell { min-height:100vh; display:grid; grid-template-columns:220px 1fr; }
.shell > nav { padding:22px 14px; background:#182330; border-right:1px solid #2b3a4b; }
.brand { margin:0 0 18px; font-size:16px; letter-spacing:.03em; }
.workspace-bridges { margin:0 0 18px; padding:10px 8px; border:1px solid #304254; border-radius:6px; background:#141e29; }
.workspace-bridges > p { margin:0 2px 5px; color:#8fa1b2; font-size:11px; letter-spacing:.08em; text-transform:uppercase; }
.workspace-bridges a { padding:5px 7px !important; margin:1px 0 !important; font-size:13px; }
.nav-group { margin:19px 10px 6px; color:#8092a5; font-size:10px; letter-spacing:.11em; text-transform:uppercase; }
.nav-section { margin:12px 0 0; }
.nav-section > summary { cursor:pointer; list-style:none; display:flex; align-items:center; justify-content:space-between; padding:5px 10px; }
.nav-section > summary::-webkit-details-marker { display:none; }
.nav-section > summary::after { content:'+'; color:#8092a5; font-size:14px; }
.nav-section[open] > summary::after { content:'−'; }
.nav-section > a, .nav-section > .nav-disabled { margin-left:4px !important; }
.shell > nav a { display:flex; align-items:center; justify-content:space-between; gap:8px; padding:8px 10px; margin:3px 0; border-radius:5px; color:#c7d2de; text-decoration:none; }
.shell > nav a:hover, .shell > nav a[aria-current="page"] { background:#26394e; color:#fff; }
.operation-tabs { display:flex; flex-wrap:wrap; gap:6px; margin:12px 0 22px; padding:0 0 6px; border-bottom:1px solid #2b3a4b; }
.operation-tabs a { display:inline-block; padding:8px 10px; border-radius:5px 5px 0 0; color:#c7d2de; text-decoration:none; }
.operation-tabs a:hover, .operation-tabs a[aria-current="page"] { background:#26394e; color:#fff; }
.nav-disabled { display:flex; justify-content:space-between; gap:8px; padding:9px 10px; margin:3px 0; color:#788797; }
.nav-status { color:#9eacba; font-size:10px; white-space:nowrap; }
.nav-status.status-unavailable { color:#ed7a80; }
.nav-status.status-healthy { color:#6fc58a; }
.nav-status.status-unknown { color:#9eacba; }
.nav-status.status-degraded { color:#d4bd69; }
.nav-status.status-planned { color:#d4bd69; }
.overview-summary { display:flex; align-items:center; justify-content:space-between; gap:10px 18px; flex-wrap:wrap; margin:0 0 10px; padding:10px 12px; border:1px solid #2b3a4b; border-radius:6px; background:#151f2a; }
.overview-summary p { margin:0; }
.overview-summary strong { color:#e6edf3; }
.overview-summary.status-unavailable { border-color:#7a3e45; }
.service-directory { width:100%; border-collapse:collapse; }
.service-directory th, .service-directory td { padding:8px 10px; border:0; border-bottom:1px solid #293747; text-align:left; }
.service-directory th { color:#9eacba; font-size:11px; font-weight:600; text-transform:uppercase; letter-spacing:.04em; }
.service-directory tr:last-child td { border-bottom:0; }
.service-directory a { color:#b9d8f4; }
.service-name { font-weight:600; }
.service-readiness { display:block; margin-top:2px; color:#9eacba; font-size:11px; }
.service-state { font-weight:650; white-space:nowrap; }
.state-healthy { color:#6fc58a; }
.state-unavailable { color:#ed7a80; }
.state-planned { color:#d4bd69; }
main { padding:30px clamp(20px,4vw,56px); max-width:1100px; }
.admin-workspace { min-width:0; }
.admin-header { display:flex; align-items:center; justify-content:space-between; gap:18px; padding:14px clamp(20px,4vw,56px); border-bottom:1px solid #2b3a4b; background:#151f2a; }
.admin-section-name { margin:2px 0 0; font-size:16px; font-weight:650; }
.admin-statuses { display:flex; align-items:center; justify-content:flex-end; gap:8px 18px; flex-wrap:wrap; }
.admin-status-item { margin:0; color:#9eacba; font-size:12px; }
.status-pill { display:inline-block; margin-left:5px; padding:2px 7px; border:1px solid #40536a; border-radius:10px; color:#c7d2de; font-size:11px; }
.status-pill.status-healthy { border-color:#316947; color:#8bd59f; }
.status-pill.status-unavailable { border-color:#7a3e45; color:#f08b92; }
.status-pill.status-protected { border-color:#53677e; color:#b4c2d1; }
.deployment-card { display:grid; gap:7px; }
.deployment-card > h3, .deployment-card > p { margin:0; }
.deployment-card-header { display:flex; align-items:center; justify-content:space-between; gap:10px; }
.deployment-card-header h3 { min-width:0; margin:0; overflow-wrap:anywhere; font-size:15px; }
.deployment-status { flex:0 0 auto; margin:0; text-transform:capitalize; }
.deployment-status.status-approved, .deployment-status.status-succeeded { border-color:#316947; color:#8bd59f; }
.deployment-status.status-failed, .deployment-status.status-rejected { border-color:#7a3e45; color:#f08b92; }
.deployment-status.status-executing { border-color:#806a2f; color:#e5cb78; }
.deployment-card details { margin-top:2px; }
.deployment-card details > p { overflow-wrap:anywhere; }
.history-controls { display:grid; grid-template-columns:minmax(220px,2fr) repeat(2,minmax(150px,1fr)); gap:10px; align-items:end; margin:12px 0; }
.history-controls label { margin:0; }
.history-count { margin:8px 0 12px; }
.history-toolbar { display:flex; justify-content:space-between; align-items:center; gap:10px; flex-wrap:wrap; margin:10px 0; }
.history-toolbar p { margin:0; }
.history-workbench { display:grid; grid-template-columns:minmax(260px,.9fr) minmax(0,1.6fr); border:1px solid #2b3a4b; border-radius:7px; overflow:hidden; background:#151f2a; }
.history-record-list { list-style:none; margin:0; padding:0; max-height:68vh; overflow:auto; border-right:1px solid #2b3a4b; }
.history-record-list li + li { border-top:1px solid #293747; }
.history-record { display:grid; width:100%; gap:4px; padding:10px 12px; border:0; border-radius:0; text-align:left; background:transparent; }
.history-record:hover, .history-record[aria-current="true"] { background:#26394e; }
.history-record-title { overflow:hidden; color:#e6edf3; font-size:13px; font-weight:600; text-overflow:ellipsis; white-space:nowrap; }
.history-record-meta { display:flex; flex-wrap:wrap; gap:4px 9px; color:#9eacba; font-size:11px; }
.history-record-detail { min-width:0; min-height:250px; padding:16px; overflow:auto; }
.history-record-detail > header { display:flex; align-items:flex-start; justify-content:space-between; gap:10px; }
.history-record-detail h2 { margin:0; overflow-wrap:anywhere; font-size:17px; }
.history-source-status { display:flex; flex-wrap:wrap; gap:6px 12px; margin:10px 0; color:#9eacba; font-size:11px; }
.history-source-status span { white-space:nowrap; }
.history-source-status .source-error { color:#ed9aa0; }
.history-source-panel { margin:10px 0; border-top:1px solid #2b3a4b; border-bottom:1px solid #2b3a4b; }
.history-source-panel > summary { padding:7px 0; cursor:pointer; color:#9eacba; font-size:12px; }
.request-queue-table { width:100%; margin-top:8px; border-collapse:collapse; }
.request-queue-table th, .request-queue-table td { padding:8px 10px; border:0; border-bottom:1px solid #293747; text-align:left; }
.request-queue-table th { color:#9eacba; font-size:11px; font-weight:600; text-transform:uppercase; letter-spacing:.04em; }
.request-queue-table tr:last-child td { border-bottom:0; }
.queue-count { color:#e6edf3; font-variant-numeric:tabular-nums; font-weight:650; }
.queue-action { white-space:nowrap; }
.queue-action a { color:#b9d8f4; }
.request-queue-header { display:flex; align-items:center; justify-content:space-between; gap:10px; }
.request-queue-header h2 { margin:0; }
.history-meta { display:flex; flex-wrap:wrap; gap:6px 12px; margin:6px 0; color:#9eacba; font-size:12px; }
.history-kind { color:#8fc5f4; font-size:11px; font-weight:650; text-transform:uppercase; letter-spacing:.06em; }
.history-state { border:1px solid #40536a; border-radius:10px; padding:1px 7px; white-space:nowrap; font-size:11px; }
.history-state.state-good { border-color:#316947; color:#8bd59f; }
.history-state.state-warning { border-color:#806a2f; color:#e5cb78; }
.approval-state-warning { color:#e5cb78; }
.history-state.state-danger { border-color:#7a3e45; color:#f08b92; }
.history-detail { margin:7px 0 0; overflow-wrap:anywhere; }
.history-detail-list { margin:6px 0 0; padding-left:18px; color:#c7d2de; }
.ssh-workbench { display:grid; grid-template-columns:minmax(260px, .78fr) minmax(360px, 1.5fr); gap:14px; align-items:start; }
.ssh-list-panel, .ssh-detail-panel { min-width:0; background:#182330; border:1px solid #2b3a4b; border-radius:7px; }
.ssh-list-panel { overflow:hidden; }
.ssh-list-toolbar { padding:12px; border-bottom:1px solid #2b3a4b; }
.ssh-list-toolbar label { margin:0 0 8px; }
.ssh-list-toolbar input, .ssh-list-toolbar select { width:100%; }
.ssh-list-count { margin:8px 0 0; color:#9eacba; font-size:12px; }
.ssh-record-list { list-style:none; padding:0; margin:0; max-height:66vh; overflow:auto; }
.ssh-record-list li + li { border-top:1px solid #2b3a4b; }
.ssh-record { display:block; width:100%; padding:11px 12px; border:0; border-radius:0; text-align:left; background:transparent; }
.ssh-record:hover, .ssh-record[aria-current="true"] { background:#26394e; }
.ssh-record strong, .ssh-record span { display:block; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
.ssh-record strong { font-size:13px; }
.ssh-record span { margin-top:3px; color:#9eacba; font-size:11px; }
.ssh-record .ssh-record-state { color:#8fc5f4; text-transform:capitalize; }
.ssh-detail-panel { padding:18px; }
.ssh-detail-header { display:flex; justify-content:space-between; align-items:flex-start; gap:12px; border-bottom:1px solid #2b3a4b; padding-bottom:12px; }
.ssh-detail-header h2 { margin:0; overflow-wrap:anywhere; font-size:18px; }
.ssh-detail-header p { margin:5px 0 0; }
.ssh-facts { display:grid; grid-template-columns:repeat(2,minmax(0,1fr)); gap:10px; margin:14px 0; }
.ssh-fact { min-width:0; padding:10px; background:#111821; border-radius:5px; }
.ssh-fact dt { color:#9eacba; font-size:11px; }
.ssh-fact dd { margin:3px 0 0; overflow-wrap:anywhere; }
.ssh-action-row { display:flex; gap:8px; align-items:center; flex-wrap:wrap; margin-top:14px; }
.ssh-action-row label { margin:0; }
.ssh-history { margin-top:14px; border-top:1px solid #2b3a4b; padding-top:10px; }
.ssh-history-list { max-height:260px; overflow:auto; }
.ssh-history-list p { margin:7px 0; padding-left:10px; border-left:2px solid #40536a; }
.ssh-pending-note { padding:10px; border-left:3px solid #d4bd69; background:#28271e; }
.recovery-facts { display:grid; grid-template-columns:repeat(auto-fit,minmax(190px,1fr)); gap:10px; margin:14px 0 0; }
.recovery-actions { display:flex; flex-wrap:wrap; gap:8px; }
.recovery-details { margin-top:8px; border-top:1px solid var(--line); padding-top:8px; }
.recovery-details > summary { cursor:pointer; color:var(--muted); font-size:13px; }
.recovery-fact { min-width:0; padding:11px; background:#111821; border-radius:5px; }
.recovery-fact dt { color:#9eacba; font-size:11px; }
.recovery-fact dd { margin:4px 0 0; overflow-wrap:anywhere; font-weight:600; }
.recovery-fact dd.state-warning { color:#e5cb78; }
.recovery-fact dd.state-danger { color:#f08b92; }
.recovery-fact dd.state-success { color:#93d9a7; }
.recovery-fact dd { overflow-wrap:anywhere; }
.approval-source { margin:10px 0; background:#182330; border:1px solid #2b3a4b; border-radius:7px; overflow:hidden; }
.approval-source > summary { display:flex; justify-content:space-between; align-items:center; gap:12px; padding:12px 14px; cursor:pointer; list-style:none; font-weight:650; }
.approval-source > summary::-webkit-details-marker { display:none; }
.approval-source > summary::after { content:'+'; order:2; margin-left:auto; color:#8092a5; }
.approval-source[open] > summary::after { content:'−'; }
.approval-source-count { order:1; margin-left:auto; color:#9eacba; font-size:12px; font-weight:500; }
.approval-source-body { padding:0 14px 12px; }
.approval-deploy { padding:10px 12px; margin:8px 0; }
.approval-deploy-header { display:flex; justify-content:space-between; align-items:flex-start; gap:10px; }
.approval-deploy-header h3 { margin:0; font-size:14px; overflow-wrap:anywhere; }
.approval-deploy-primary { display:flex; flex-wrap:wrap; gap:6px 12px; align-items:center; margin:5px 0; }
.approval-deploy-primary p { margin:0; }
.approval-deploy-summary { margin:5px 0; color:#cbd6df; }
.approval-deploy-details { margin-top:5px; font-size:12px; }
.approval-deploy-actions { display:flex; gap:6px; align-items:center; margin-top:8px; }
.approval-deploy-actions button { padding:4px 8px; font-size:12px; }
.approval-state { display:inline-flex; align-items:center; padding:2px 7px; border:1px solid #40536a; border-radius:999px; white-space:nowrap; font-size:11px; }
.approval-state.state-success { color:#93d9a7; border-color:#39754a; }
.approval-state.state-warning { color:#e5cb78; border-color:#7a6932; }
.approval-state.state-danger { color:#f08b92; border-color:#83444a; }
.approval-state.state-muted { color:#9eacba; }
.approval-deploy-details > summary { cursor:pointer; color:#9eacba; font-size:12px; }
.approval-deploy-details p { margin:6px 0; overflow-wrap:anywhere; }
.server-fleet-toolbar { display:flex; justify-content:space-between; align-items:center; gap:10px; }
.server-fleet-grid { display:grid; grid-template-columns:repeat(auto-fit,minmax(280px,1fr)); gap:10px; }
.server-node { padding:12px; }
.server-node-select { display:block; width:100%; padding:0; border:0; color:inherit; text-align:left; background:transparent; }
.server-node-select[aria-pressed="true"] { outline:2px solid #7bc6a4; outline-offset:3px; border-radius:4px; }
.server-node-header { display:flex; justify-content:space-between; align-items:center; gap:10px; }
.server-node-header h3 { margin:0; font-size:15px; }
.server-node-role { color:#9eacba; font-size:11px; text-transform:uppercase; letter-spacing:.06em; }
.server-node-metrics { display:grid; grid-template-columns:repeat(2,minmax(0,1fr)); gap:8px; margin:10px 0 0; }
.server-node-metrics > div { min-width:0; background:#111821; border-radius:4px; padding:8px; }
.server-node-metrics dt { color:#9eacba; font-size:11px; }
.server-node-metrics dd { margin:3px 0 0; font-size:14px; font-variant-numeric:tabular-nums; overflow-wrap:anywhere; }
.server-node-status { color:#93d9a7; font-size:12px; }
.server-node-status.stale { color:#e5cb78; }
.server-node-status.unavailable { color:#f08b92; }
.server-fleet-note { margin:8px 0 0; color:#9eacba; font-size:12px; }
.server-network-interfaces { display:grid; grid-template-columns:repeat(auto-fit,minmax(220px,1fr)); gap:8px; }
.server-network-interface { border:1px solid #33414e; border-radius:6px; padding:8px 10px; background:#111820; }
.server-network-interface p { margin:4px 0 0; font-size:12px; color:#b7c3cf; }
.server-network-toolbar { display:flex; align-items:center; gap:8px; flex-wrap:wrap; }
.server-network-chart { display:block; width:100%; height:120px; margin:10px 0; border:1px solid #33414e; border-radius:6px; background:#111820; }
.server-node-connect-form { display:flex; flex-wrap:wrap; align-items:end; gap:10px; margin:12px 0; }
.server-node-form { padding:12px; margin:12px 0; border:1px solid #34404e; border-radius:6px; background:#111821; }
.server-node-form h4 { flex-basis:100%; margin:0; }
.server-node-form p { flex-basis:100%; }
.server-node-connect-form label { display:grid; gap:4px; min-width:180px; color:#aebbc8; font-size:12px; }
.server-node-secret { margin:12px 0; padding:12px; border:1px solid #8c7a3e; border-radius:6px; background:#19180f; }
.server-node-secret pre { padding:10px; overflow-wrap:anywhere; white-space:pre-wrap; user-select:all; background:#10151b; }
.server-node-management-row { display:flex; justify-content:space-between; align-items:center; gap:12px; padding:10px 0; border-top:1px solid #29323d; }
.server-node-management-row p { margin:4px 0 0; font-size:12px; overflow-wrap:anywhere; }
.server-node-events { margin-top:12px; }
.server-node-events li { margin:6px 0; color:#aebbc8; font-size:12px; overflow-wrap:anywhere; }
.server-room-tabs { display:flex; gap:4px; flex-wrap:wrap; margin:0 0 14px; border-bottom:1px solid #29323d; }
.server-room-tabs [role="tab"] { border:0; border-bottom:2px solid transparent; border-radius:0; padding:8px 12px; color:#aebbc8; background:transparent; }
.server-room-tabs [role="tab"][aria-selected="true"] { color:#e5edf4; border-bottom-color:#7bc6a4; }
.server-room-panel[hidden], .server-node-form[hidden], .server-node-secret[hidden] { display:none !important; }
.server-room-selected { display:flex; justify-content:space-between; align-items:center; gap:12px; margin-bottom:8px; }
.server-room-selected h2 { margin:0; }
.server-room-selected p { margin:0; }
.server-selected-overview .server-node-metrics { grid-template-columns:repeat(auto-fit,minmax(140px,1fr)); }
.server-node-records { margin-top:16px; }
.server-room-actions { display:grid; grid-template-columns:repeat(auto-fit,minmax(180px,1fr)); gap:8px; margin:12px 0; }
.server-room-actions > button { min-height:42px; text-align:left; }
.server-room-actions small { display:block; margin-top:3px; color:#aebbc8; font-size:11px; font-weight:400; }
.server-node-setup-steps { display:grid; gap:4px; margin:8px 0; color:#b7c7d4; }
.server-node-secret pre[hidden] { display:none !important; }
.server-operation-controls { display:flex; align-items:end; flex-wrap:wrap; gap:8px; margin:10px 0; }
.server-operation-controls label { display:grid; gap:4px; min-width:220px; margin:0; color:#aebbc8; font-size:12px; }
.server-module-actions { display:grid; grid-template-columns:repeat(auto-fit,minmax(150px,1fr)); gap:6px; margin:10px 0; }
.server-module-actions button { min-height:38px; text-align:left; }
.server-operation-list { display:grid; gap:6px; margin-top:10px; }
.server-operation-item { display:grid; grid-template-columns:minmax(120px,1fr) auto; align-items:start; gap:4px 10px; padding:8px 0; border-top:1px solid #29323d; }
.server-operation-item small { grid-column:1/-1; color:#9eacba; overflow-wrap:anywhere; }
.server-operation-state { color:#93d9a7; font-size:11px; text-transform:uppercase; }
.server-operation-state.failed,.server-operation-state.outcome_unknown { color:#f08b92; }
.server-operation-state.running,.server-operation-state.accepted,.server-operation-state.awaiting_reboot { color:#e5cb78; }
.table-scroll { max-width:100%; overflow-x:auto; }
.table-scroll:focus-visible { outline:2px solid #8fc5f4; outline-offset:2px; }
.skip-link { position:absolute; left:-9999px; top:8px; z-index:5; padding:8px 12px; background:#fff; color:#111; }
.skip-link:focus { left:8px; }
a:focus-visible, button:focus-visible, input:focus-visible, select:focus-visible { outline:2px solid #8fc5f4; outline-offset:2px; }
.eyebrow { color:#80a8cd; text-transform:uppercase; letter-spacing:.12em; font-size:11px; }
h1 { margin:.35rem 0 1.25rem; font-size:26px; }
.page-intro { max-width:760px; margin:-8px 0 22px; color:#aebdc3; line-height:1.65; }
.button-link { display:inline-flex; min-height:38px; align-items:center; padding:0 13px; border-radius:5px; background:#9fe1ce; color:#10201d; font-weight:700; text-decoration:none; }
.button-link:hover { background:#b8f0df; }
.card { background:#182330; border:1px solid #2b3a4b; border-radius:7px; padding:16px; }
.muted { color:#9eacba; }
table { border-collapse:collapse; width:100%; margin:12px 0 24px; }
td, th { border:1px solid #2b3a4b; padding:7px 9px; text-align:left; vertical-align:top; }
th { background:#202f3f; color:#d8e3ed; }
input, select, button { font:inherit; color:inherit; background:#111821; border:1px solid #40536a; border-radius:4px; padding:5px 7px; }
button { cursor:pointer; background:#26394e; }
label { display:block; margin:8px 0; }
form.inline { display:inline; }
.notice { background:#163222; border-left:3px solid #42a56b; padding:8px 10px; }
.error { background:#3a2024; border-left:3px solid #da626c; padding:8px 10px; }
@media(max-width:820px) { .ssh-workbench { grid-template-columns:1fr; } .ssh-record-list { max-height:36vh; } }
@media(max-width:720px) { .shell { grid-template-columns:1fr; } .shell > nav { border-right:0; border-bottom:1px solid #2b3a4b; padding:12px; } .nav-group { display:inline-block; margin:8px 8px 4px; } .shell > nav a { display:inline-flex; } .admin-header { align-items:flex-start; flex-direction:column; padding:12px 14px; } .admin-statuses { justify-content:flex-start; } main { padding:20px 14px; } }
@media(max-width:620px) { .history-controls { grid-template-columns:1fr; } }
@media(max-width:760px) { .history-workbench { grid-template-columns:1fr; } .history-record-list { max-height:30vh; border-right:0; border-bottom:1px solid #2b3a4b; } }
"#;

const JS: &str = r#"
(() => {
  const main = document.querySelector('main#admin-content');
  if (!main) return;
  document.querySelectorAll('time[data-unix]').forEach(element => {
    const seconds = Number(element.dataset.unix);
    if (Number.isFinite(seconds) && seconds > 0 && seconds <= 8640000000000) {
      const date = new Date(seconds * 1000);
      element.dateTime = date.toISOString();
      element.textContent = date.toLocaleString();
    }
  });
  const responseFailure = async (response, fallback) => {
    let detail = '';
    try { detail = (await response.text()).trim(); } catch (_) {}
    detail = detail.replace(/[\u0000-\u001f\u007f]/g, ' ').slice(0, 240);
    if (response.status === 401) return 'Sign in with an Identity root account to review temporary SSH requests.';
    if (response.status === 403) return 'Your signed-in account does not have the Platform root role.';
    return `${fallback} (HTTP ${response.status})${detail ? `: ${detail}` : ''}`;
  };
  const capabilityLabel = value => ({
    vapor_deploy: 'Deploy Vapor updates',
    vapor_service_control: 'Start and manage Vapor services',
    host_admin: 'Temporary administrator access (unrestricted sudo)'
  })[value] || String(value || '').replaceAll('_', ' ');
  const capabilityList = values => (Array.isArray(values) ? values : []).map(capabilityLabel).join(', ');
  const formatDuration = seconds => {
    let remaining = Math.max(0, Math.floor(Number(seconds) || 0));
    const units = [['d', 86400], ['h', 3600], ['m', 60], ['s', 1]];
    const parts = [];
    for (const [label, size] of units) {
      const amount = Math.floor(remaining / size);
      if (amount || (label === 's' && !parts.length)) parts.push(`${amount}${label}`);
      remaining %= size;
    }
    return parts.slice(0, 2).join(' ');
  };
  const showResponseFailure = async (container, response, fallback) => {
    let message = container.querySelector(':scope > .ssh-operation-error');
    if (!message) {
      message = document.createElement('p');
      message.className = 'error ssh-operation-error';
      message.setAttribute('role', 'alert');
      container.prepend(message);
    }
    message.textContent = await responseFailure(response, fallback);
  };
  const updateShellStatus = (response, body) => {
    const operator = document.querySelector('[data-operator-session]');
    const platform = document.querySelector('[data-platform-status]');
    const setPill = (element, label, state) => {
      if (!element) return;
      element.textContent = label;
      element.className = `status-pill status-${state}`;
    };
    const updateNavHealth = services => {
      for (const link of document.querySelectorAll('[data-admin-services]')) {
        const status = link.querySelector('[data-nav-status]');
        if (!status) continue;
        const requiredIds = link.dataset.adminServices.split(',').filter(Boolean);
        const required = requiredIds.map(id => services && services.find(item => item.id === id));
        if (!required.length || required.some(service => !service ||
            (service.health !== 'healthy' && service.health !== 'unavailable'))) {
          status.textContent = 'Unknown';
          status.className = 'nav-status status-unknown';
          continue;
        }
        const healthyCount = required.filter(service => service.health === 'healthy').length;
        const state = healthyCount === required.length ? 'healthy' :
          healthyCount === 0 ? 'unavailable' : 'degraded';
        status.textContent = state === 'healthy' ? 'Healthy' :
          state === 'unavailable' ? 'Offline' : 'Degraded';
        status.className = `nav-status status-${state}`;
      }
    };
    if (!response) {
      setPill(operator, 'Unavailable', 'unavailable');
      setPill(platform, 'Unavailable', 'unavailable');
      updateNavHealth(null);
      return;
    }
    if (!response.ok) {
      const label = response.status === 401 ? 'Sign in required' :
        response.status === 403 ? 'Not an operator' : 'Unavailable';
      const state = response.status === 401 || response.status === 403 ? 'protected' : 'unavailable';
      setPill(operator, label, state);
      setPill(platform, response.status === 401 || response.status === 403 ? 'Access restricted' : 'Unavailable', state);
      updateNavHealth(null);
      return;
    }

    setPill(operator, 'Authorized', 'healthy');
    const services = Array.isArray(body && body.services) ? body.services : [];
    const platformService = services.find(service => service.id === 'platform');
    const platformState = platformService &&
      (platformService.health === 'healthy' || platformService.health === 'unavailable')
      ? platformService.health : 'unknown';
    const platformLabel = platformState === 'healthy' ? 'Healthy' :
      platformState === 'unavailable' ? 'Offline' : 'Unknown';
    setPill(platform, platformLabel, platformState);
    updateNavHealth(services);
  };
  const overview = document.querySelector('#platform-overview, #operations-status');
  const hasServiceBadges = document.querySelector('[data-admin-services]');
  if (overview || hasServiceBadges) {
    fetch('/api/admin/overview', { credentials: 'same-origin', cache: 'no-store' })
      .then(async response => ({ response, body: await response.json().catch(() => null) }))
      .then(({ response, body }) => {
        updateShellStatus(response, body);
        if (!overview) return;
        overview.replaceChildren();
        if (!response.ok) {
          const message = document.createElement('p');
          message.className = 'muted';
          message.textContent = response.status === 401 ? 'Sign in with Steam. Platform status is visible to authorized operators.' :
            response.status === 403 ? 'This account is signed in but is not authorized for Platform operations.' :
            'Service status is temporarily unavailable.';
          overview.append(message);
          if (response.status === 401) {
            const login = document.createElement('a');
            login.href = '/login';
            login.textContent = 'Continue with Steam';
            overview.append(login);
          }
          return;
        }
        const services = body && Array.isArray(body.services) ? body.services : [];
        const unavailable = services.filter(service => service.health !== 'healthy');
        const summary = document.createElement('div');
        summary.className = `overview-summary${unavailable.length ? ' status-unavailable' : ''}`;
        summary.setAttribute('role', 'status');
        const summaryText = document.createElement('p');
        const state = document.createElement('strong');
        state.textContent = !services.length ? 'No service status reported' : unavailable.length ? `${unavailable.length} service${unavailable.length === 1 ? '' : 's'} need attention` : 'All service probes healthy';
        summaryText.append(state, document.createTextNode(` · ${services.length} services · revision ${body.deployed_revision || 'not recorded'}`));
        summary.append(summaryText);
        overview.append(summary);

        if (overview.id === 'platform-overview') {
          const hosting = document.createElement('a');
          hosting.href = '/admin/hosting';
          hosting.textContent = 'View service details';
          overview.append(hosting);
          return;
        }

        const table = document.createElement('table');
        table.className = 'service-directory';
        const head = document.createElement('thead');
        head.innerHTML = '<tr><th scope="col">Service</th><th scope="col">Health</th><th scope="col">Work area</th></tr>';
        const bodyRows = document.createElement('tbody');
        const rank = health => health === 'unavailable' ? 0 : health === 'unknown' ? 1 : 2;
        for (const service of services.slice().sort((a, b) => rank(a.health) - rank(b.health) || String(a.name).localeCompare(String(b.name)))) {
          const row = document.createElement('tr');
          const nameCell = document.createElement('td');
          nameCell.className = 'service-name';
          nameCell.textContent = service.name;
          if (service.readiness && service.readiness !== 'not reported') {
            const readiness = document.createElement('span');
            readiness.className = 'service-readiness';
            readiness.textContent = `Readiness · ${service.readiness}`;
            nameCell.append(readiness);
          }
          const healthCell = document.createElement('td');
          healthCell.className = `service-state state-${service.health}`;
          healthCell.textContent = service.health === 'healthy' ? 'Healthy' : service.health === 'unavailable' ? 'Offline' : 'Unknown';
          const actionCell = document.createElement('td');
          if (service.admin_url) {
            const link = document.createElement('a');
            link.href = service.admin_url;
            link.textContent = 'Open';
            actionCell.append(link);
          } else {
            actionCell.textContent = '—';
            actionCell.className = 'muted';
          }
          row.append(nameCell, healthCell, actionCell);
          bodyRows.append(row);
        }
        table.append(head, bodyRows);
        overview.append(table);
      })
      .catch(() => {
        updateShellStatus(null, null);
        overview.textContent = 'Service status is temporarily unavailable.';
      });
  } else {
    fetch('/api/admin/overview', { credentials: 'same-origin', cache: 'no-store' })
      .then(async response => ({ response, body: await response.json().catch(() => null) }))
      .then(({ response, body }) => updateShellStatus(response, body))
      .catch(() => updateShellStatus(null, null));
  }

  const requestQueue = document.querySelector('#operator-queue');
  if (requestQueue) {
    const refreshQueueButton = document.querySelector('#operator-queue-refresh');
    const sources = [
      { name: 'Deployments', url: '/api/admin/deploy-requests?limit=200', href: '/admin/approvals', summarize: items => ({ primary: items.filter(item => item.status === 'pending' && typeof item.current_main_commit === 'string' && String(item.commit || '').toLowerCase() === String(item.current_main_commit).toLowerCase()).length, label: 'Awaiting approval', secondary: items.filter(item => item.status === 'approved' && ['failed', 'unknown', 'dispatching'].includes(item.dispatch_state)).length, secondaryLabel: 'Execution follow-up' }) },
      { name: 'Temporary access', url: '/api/admin/ssh-access/requests?limit=200', href: '/admin/approvals', summarize: items => ({ primary: items.filter(item => item.status === 'pending').length, label: 'Awaiting review', secondary: items.filter(item => item.status === 'active').length, secondaryLabel: 'Active grants' }) },
      { name: 'Identity permissions', url: '/api/identity/v1/admin/permission-requests/page?limit=200', href: '/admin/approvals', summarize: items => { const pending = items.filter(item => item.status === 'pending'); const overdue = pending.filter(item => Number(item.expires_at_unix) > 0 && Number(item.expires_at_unix) <= Math.floor(Date.now() / 1000)).length; return { primary: pending.length - overdue, label: 'Awaiting review', secondary: overdue, secondaryLabel: 'Expired by time; owner transition pending' }; } }
    ];
    const renderQueue = async () => {
      if (refreshQueueButton?.disabled) return;
      if (refreshQueueButton) { refreshQueueButton.disabled = true; refreshQueueButton.textContent = 'Refreshing…'; }
      requestQueue.replaceChildren();
      const table = document.createElement('table'); table.className = 'request-queue-table';
      table.innerHTML = '<thead><tr><th scope="col">Workflow</th><th scope="col">Awaiting decision</th><th scope="col">Execution follow-up</th><th scope="col">Open</th></tr></thead>';
      const body = document.createElement('tbody');
      const outcomes = await Promise.all(sources.map(async source => {
        try {
          const response = await fetch(source.url, { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(6000) });
          const items = await response.json().catch(() => null);
          if (!response.ok) return { source, error: response.status === 401 ? 'Sign in with Steam' : response.status === 403 ? 'Not an operator' : `Unavailable (HTTP ${response.status})` };
          const records = Array.isArray(items) ? items : items?.items;
          if (!Array.isArray(records)) return { source, error: 'Invalid service response' };
          const moreAvailable = !Array.isArray(items) && typeof items?.next_cursor === 'string' && items.next_cursor.length > 0;
          return { source, counts: source.summarize(records), moreAvailable };
        } catch (_) { return { source, error: 'Could not contact service' }; }
      }));
      for (const outcome of outcomes) {
        const row = document.createElement('tr');
        const heading = document.createElement('td'); heading.textContent = outcome.source.name; row.append(heading);
        const primary = document.createElement('td'); primary.className = 'queue-count';
        const secondary = document.createElement('td'); secondary.className = 'muted';
        let actionHref = outcome.source.href;
        let actionText = 'Open';
        if (outcome.error) {
          primary.textContent = '—';
          secondary.textContent = outcome.error;
        } else {
          primary.textContent = `${outcome.counts.primary}${outcome.moreAvailable ? ' shown' : ''}`;
          secondary.textContent = outcome.counts.secondary > 0 ? `${outcome.counts.secondary}${outcome.moreAvailable ? ' shown' : ''} · ${outcome.counts.secondaryLabel}` : outcome.moreAvailable ? 'Older records available' : '—';
          if (outcome.counts.primary > 0) actionText = 'Review';
          if (outcome.source.name === 'Deployments' && outcome.counts.primary === 0 && outcome.counts.secondary > 0) { actionHref = '/admin/deployments'; actionText = 'Inspect'; }
          if (outcome.source.name === 'Temporary access' && outcome.counts.primary === 0) { actionHref = '/admin/ssh-access'; actionText = 'Manage'; }
        }
        const action = document.createElement('td'); action.className = 'queue-action';
        const link = document.createElement('a'); link.href = actionHref; link.textContent = actionText; action.append(link);
        row.append(primary, secondary, action);
        body.append(row);
      }
      table.append(body);
      requestQueue.replaceChildren(table);
      if (refreshQueueButton) { refreshQueueButton.disabled = false; refreshQueueButton.textContent = 'Refresh'; }
    };
    refreshQueueButton?.addEventListener('click', renderQueue);
    renderQueue();
  }

  const latestOperation = document.querySelector('#latest-operation-status');
  if (latestOperation) {
    fetch('/api/admin/operations', { credentials: 'same-origin', cache: 'no-store' })
      .then(async response => ({ response, body: await response.json().catch(() => null) }))
      .then(({ response, body }) => {
        latestOperation.replaceChildren();
        if (!response.ok) {
          latestOperation.textContent = response.status === 401 ? 'Sign in to view the latest operation result.' :
            response.status === 403 ? 'Root access is required to view the latest operation result.' :
            'The latest operation result is temporarily unavailable.';
          return;
        }
        if (!body || body.record_status !== 'reported' || !body.record) {
          latestOperation.textContent = body && body.record_status === 'invalid' ?
            'The latest operation record is invalid.' : 'No latest operation result is reported.';
          return;
        }
        const record = body.record;
        const title = document.createElement('h3');
        title.textContent = `${record.operation === 'deploy' ? 'Latest deployment' : 'Latest restore'}: ${record.result}`;
        const details = document.createElement('p');
        details.className = 'muted';
        details.textContent = `Revision ${record.revision} · ${record.server_mode} · started ${record.started_at_utc} · finished ${record.finished_at_utc}`;
        latestOperation.append(title, details);
      })
      .catch(() => {
        latestOperation.textContent = 'The latest operation result is temporarily unavailable.';
      });
  }

  const pendingDeployApprovals = document.querySelector('#pending-deploy-approvals');
  const pendingSshApprovals = document.querySelector('#pending-ssh-approvals');
  const pendingIdentityApprovals = document.querySelector('#pending-identity-approvals');
  if (pendingDeployApprovals && pendingSshApprovals && pendingIdentityApprovals) {
    const feedback = document.querySelector('#approvals-feedback');
    const refreshButton = document.querySelector('#approvals-refresh');
    const containers = {
      deploy: pendingDeployApprovals,
      ssh: pendingSshApprovals,
      identity: pendingIdentityApprovals
    };
    const pageButtons = {
      deploy: document.querySelector('#approvals-more-deploy'),
      ssh: document.querySelector('#approvals-more-ssh'),
      identity: document.querySelector('#approvals-more-identity')
    };
    const countBadges = {
      deploy: document.querySelector('#pending-deploy-count'),
      ssh: document.querySelector('#pending-ssh-count'),
      identity: document.querySelector('#pending-identity-count')
    };
    const sourceDefinitions = [
      { key: 'deploy', url: '/api/admin/deploy-requests?limit=200', label: 'Platform deployment approvals' },
      { key: 'ssh', url: '/api/admin/ssh-access/requests?limit=200', label: 'Platform SSH access requests' },
      { key: 'identity', url: '/api/identity/v1/admin/permission-requests/page?limit=200', label: 'Identity permission requests' }
    ];
    const sourceState = Object.fromEntries(sourceDefinitions.map(source => [source.key, { records: [], cursor: null, generation: 0 }]));
    const endpoint = (kind, id, action) => {
      const encodedId = encodeURIComponent(id);
      if (kind === 'deploy') return `/api/admin/deploy-requests/${encodedId}/${action}`;
      if (kind === 'ssh') return `/api/admin/ssh-access/requests/${encodedId}/${action}`;
      return `/api/identity/v1/admin/permission-requests/${encodedId}/${action}`;
    };
    const responseMessage = async (response, fallback) => {
      const body = await response.clone().json().catch(() => null);
      const detail = body?.error?.message || body?.message || (await response.text().catch(() => '')).trim();
      const clean = String(detail || '').replace(/[\u0000-\u001f\u007f]/g, ' ').slice(0, 240);
      if (response.status === 401) return 'Sign in with an Identity root account to review approvals.';
      if (response.status === 403) return 'Your signed-in account is not authorized to review this request.';
      return `${fallback} (HTTP ${response.status})${clean ? `: ${clean}` : ''}`;
    };
    const showDeploymentFeedback = message => {
      feedback.replaceChildren(document.createTextNode(`${message} `));
      const link = document.createElement('a'); link.href = '/admin/deployments';
      link.textContent = 'Open Deployments'; feedback.append(link);
    };
    const deploymentApprovalMessage = body => {
      if (body?.status === 'executing' || body?.dispatch?.status === 'claimed')
        return 'Approval saved; the worker claimed the deployment and it is running.';
      if (body?.status === 'succeeded') return 'Approval saved; deployment succeeded.';
      if (body?.status === 'failed') return 'Approval saved; deployment failed.';
      if (body?.dispatch?.status === 'queued') return 'Approval saved; deployment is queued for the worker.';
      if (body?.dispatch?.status === 'dispatching') return 'Approval saved; deployment dispatch is in progress.';
      if (body?.dispatch?.status === 'unknown') return 'Approval saved; dispatch result is unknown.';
      if (body?.dispatch?.status === 'failed') return 'Approval saved; dispatch failed before the worker claimed the deployment.';
      return 'Approval saved; check the current deployment stage.';
    };
    const dispatchProblem = code => ({
      dispatch_result_unknown: 'has an unknown result',
      dispatch_attempt_superseded: 'was superseded by a newer request state',
      dispatch_state_unavailable: 'could not read the current state'
    })[code] || 'failed';
    const appendText = (parent, tag, text, className) => {
      const node = document.createElement(tag);
      if (className) node.className = className;
      node.textContent = text;
      parent.append(node);
      return node;
    };
    const requestMetadata = content => {
      const details = document.createElement('details');
      details.className = 'request-metadata';
      appendText(details, 'summary', 'Request metadata');
      details.append(content);
      return details;
    };
    const humanTime = value => {
      const seconds = Number(value);
      return Number.isFinite(seconds) && seconds > 0 && seconds <= 8640000000000
        ? new Date(seconds * 1000).toLocaleString() : 'not recorded';
    };
    const deployIsSuperseded = item => {
      const current = typeof item?.current_main_commit === 'string' ? item.current_main_commit.toLowerCase() : null;
      return current !== null && String(item?.commit || '').toLowerCase() !== current;
    };
    const serviceDeployTargets = new Set((document.querySelector('[data-deployment-targets]')?.dataset.deploymentTargets || '').split(',').filter(Boolean));
    const validDeploymentSelection = item => {
      if (!['monolith', 'services'].includes(item.server_mode)) return false;
      if (item.deployment_scope === 'full') return item.service_target === null;
      return item.deployment_scope === 'service' && item.server_mode === 'services' && serviceDeployTargets.has(item.service_target);
    };
    const deploymentSelectionLabel = item => {
      if (!validDeploymentSelection(item)) return 'missing or invalid (cannot be approved)';
      if (item.deployment_scope === 'full') return `Full ${item.server_mode} deployment`;
      return `Service: ${item.service_target}`;
    };
    const isExpiredByTime = item => Number(item.expires_at_unix) > 0
      && Number(item.expires_at_unix) <= Math.floor(Date.now() / 1000);
    const requireConfirmation = checkbox => {
      checkbox.required = true;
      checkbox.addEventListener('invalid', () => {
        feedback.textContent = 'Confirm this action before submitting the request.';
      });
      return checkbox;
    };
    const confirmationForm = (item, labelText, buttonText, submit) => {
      const form = document.createElement('form');
      form.className = 'inline';
      const label = document.createElement('label');
      const confirm = document.createElement('input');
      confirm.type = 'checkbox';
      requireConfirmation(confirm);
      label.append(confirm, document.createTextNode(` ${labelText}`));
      const button = document.createElement('button');
      button.type = 'submit';
      button.textContent = buttonText;
      form.append(label, button);
      form.addEventListener('submit', event => {
        event.preventDefault();
        if (confirm.checked) submit(button);
      });
      return form;
    };
    const submitDecision = async (button, kind, item, action, body) => {
      button.disabled = true;
      feedback.textContent = `Saving ${action} with ${kind === 'identity' ? 'Identity' : 'Platform'}…`;
      try {
        const response = await fetch(endpoint(kind, item.id, action), {
          method: 'POST', credentials: 'same-origin', cache: 'no-store',
          headers: { 'content-type': 'application/json' }, body: JSON.stringify(body)
        });
        const resultBody = await response.clone().json().catch(() => null);
        if (kind === 'deploy' && resultBody?.error?.decision_saved === true) {
          showDeploymentFeedback(`Approval was saved, but deployment dispatch ${dispatchProblem(resultBody.error.code)}: ${String(resultBody.error.message || 'check Deployments for the current stage').slice(0, 240)}`);
        } else if (kind === 'deploy' && action === 'approve' && response.ok) {
          showDeploymentFeedback(deploymentApprovalMessage(resultBody));
        } else if (response.ok) {
          feedback.textContent = `${kind === 'identity' ? 'Identity' : 'Platform'} recorded the ${action} decision. Refreshing pending requests…`;
        } else {
          feedback.textContent = await responseMessage(response, `${action} was not confirmed`);
        }
      } catch (_) {
        feedback.textContent = 'No response was received. The request may have changed; refresh and check its owner status before retrying.';
      }
      await refreshInbox();
    };
    const renderDeploy = (item, card) => {
      card.classList.add('approval-deploy');
      const header = document.createElement('div'); header.className = 'approval-deploy-header';
      const title = document.createElement('h3'); title.textContent = `${item.repository || 'Unknown repository'} · ${item.branch || 'unknown branch'}`;
      const state = document.createElement('span'); state.className = 'approval-state';
      header.append(title, state); card.append(header);
      const selectionValid = validDeploymentSelection(item);
      const orderReady = item.approval_order_ready === true;
      const orderBlocker = item.approval_order_blocker && typeof item.approval_order_blocker.id === 'string' ? item.approval_order_blocker : null;
      const blockerStage = orderBlocker ? orderBlocker.status === 'pending' ? 'same revision pending' : orderBlocker.status === 'executing' ? 'deployment running' : ({ queued: 'queued', dispatching: 'starting', unknown: 'outcome uncertain', claimed: 'worker running', completed: 'finished', failed: 'dispatch failed', expired: 'expired before start' })[orderBlocker.dispatch_state] || orderBlocker.status : null;
      const currentMain = typeof item.current_main_commit === 'string' ? item.current_main_commit.toLowerCase() : null;
      const revisionCurrent = currentMain !== null && String(item.commit || '').toLowerCase() === currentMain;
      state.textContent = !currentMain ? 'Current revision unknown' : deployIsSuperseded(item) ? 'Superseded' : !selectionValid ? 'Invalid target' : orderReady ? 'Ready to approve' : orderBlocker ? `Blocked by ${orderBlocker.id.slice(0, 8)} · ${blockerStage}` : 'Waiting for prior deployment';
      if (orderBlocker) state.title = `Blocking request ${orderBlocker.id} · ${orderBlocker.commit} · ${orderBlocker.status}/${orderBlocker.dispatch_state}`;
      state.classList.add(!currentMain || !selectionValid ? 'state-danger' : deployIsSuperseded(item) ? 'state-muted' : orderReady ? 'state-success' : 'state-warning');
      const primary = document.createElement('div'); primary.className = 'approval-deploy-primary';
      const shortCommit = String(item.commit || 'unknown').slice(0, 12);
      const commit = appendText(primary, 'code', shortCommit); commit.title = item.commit || 'Commit unavailable';
      appendText(primary, 'span', deploymentSelectionLabel(item), selectionValid ? '' : 'error');
      if (item.created_at_unix) appendText(primary, 'span', `Requested ${humanTime(item.created_at_unix)}`, 'muted');
      card.append(primary);
      appendText(card, 'p', item.summary || 'No request summary provided.', 'approval-deploy-summary');

      const evidence = document.createElement('details'); evidence.className = 'approval-deploy-details';
      const evidenceSummary = document.createElement('summary'); evidenceSummary.textContent = 'Revision, requester, and CI evidence';
      const evidenceBody = document.createElement('div');
      appendText(evidenceBody, 'p', `Requested revision: ${item.commit || 'unknown'}${currentMain ? ` · current main: ${currentMain}` : ''}`, 'muted');
      appendText(evidenceBody, 'p', `Requester: ${item.requester_kind || 'unknown'} · profile ${item.requester_profile_id || 'unknown'} · expires ${humanTime(item.expires_at_unix)}`, 'muted');
      if (orderBlocker) appendText(evidenceBody, 'p', `Order blocker: ${orderBlocker.id} · ${orderBlocker.commit} · ${orderBlocker.status}/${orderBlocker.dispatch_state}`, 'muted');
      try {
        const ciUrl = new URL(item.workflow_url);
        if (ciUrl.protocol === 'https:' && ciUrl.hostname === 'github.com' && ciUrl.pathname.startsWith('/GHF-Studios/')) {
          const link = document.createElement('a'); link.href = ciUrl.href; link.rel = 'noopener noreferrer';
          link.textContent = 'Successful CI run'; evidenceBody.append(link);
        }
      } catch (_) {}
      const pins = document.createElement('details');
      const summary = document.createElement('summary'); summary.textContent = `${Array.isArray(item.submodules) ? item.submodules.length : 0} pinned service revisions`;
      const list = document.createElement('ul');
      for (const pin of (Array.isArray(item.submodules) ? item.submodules : [])) appendText(list, 'li', `${pin.path}: ${pin.commit}`);
      pins.append(summary, list); evidenceBody.append(pins); evidence.append(evidenceSummary, evidenceBody); card.append(evidence);
      if (!currentMain) appendText(card, 'p', 'Approval is disabled until current main can be verified.', 'approval-state-warning');
      else if (deployIsSuperseded(item)) {
        appendText(card, 'p', `This revision was superseded by main ${currentMain.slice(0, 12)}. It cannot be approved; dismiss it to clear the pending queue.`, 'approval-state-warning');
        const actions = document.createElement('div'); actions.className = 'approval-deploy-actions';
        const dismiss = document.createElement('button'); dismiss.type = 'button'; dismiss.textContent = 'Dismiss superseded';
        dismiss.addEventListener('click', () => {
          if (!window.confirm(`Reject and close superseded request ${shortCommit}? It will not be deployed.`)) return;
          submitDecision(dismiss, 'deploy', item, 'reject', { expected_version: item.version, confirmed: true, reason: 'superseded by a later main revision' });
        });
        actions.append(dismiss); card.append(actions); return;
      }
      const actions = document.createElement('div'); actions.className = 'approval-deploy-actions';
      const approve = document.createElement('button'); approve.type = 'button'; approve.textContent = 'Approve current main commit';
      approve.disabled = !currentMain || !orderReady || !selectionValid || !revisionCurrent || isExpiredByTime(item);
      approve.title = approve.disabled ? 'Approval requires a current revision, valid target, unexpired request, and correct queue order.' : `Approve ${item.commit} for ${deploymentSelectionLabel(item)}`;
      approve.addEventListener('click', () => {
        if (!window.confirm(`Queue ${item.repository || 'repository'} ${item.commit || 'unknown commit'} (${deploymentSelectionLabel(item)}) now?`)) return;
        submitDecision(approve, 'deploy', item, 'approve', { expected_version: item.version, confirmed: true, reason: 'approved current main in Platform Approvals' });
      });
      const reject = document.createElement('button'); reject.type = 'button'; reject.textContent = 'Reject';
      reject.addEventListener('click', () => {
        if (!window.confirm(`Reject ${item.repository || 'repository'} ${shortCommit}?`)) return;
        submitDecision(reject, 'deploy', item, 'reject', { expected_version: item.version, confirmed: true, reason: 'rejected in Platform Approvals' });
      });
      actions.append(approve, reject);
      card.append(actions);
    };
    const renderSsh = (item, card) => {
      appendText(card, 'h3', `${item.fingerprint || 'Unknown key'} · ${capabilityList(item.requested_capabilities) || 'Shell access'}`);
      appendText(card, 'p', item.purpose || 'No purpose provided.');
      const metadata = document.createElement('p'); metadata.className = 'muted';
      metadata.textContent = `Requested by ${item.requester_kind || 'unknown'} · profile ${item.requester_profile_id || 'unknown'}`;
      card.append(requestMetadata(metadata));
      appendText(card, 'p', `Requested ${humanTime(item.created_at_unix)} · duration ${formatDuration(item.requested_duration_seconds)}`, 'muted');
      const form = document.createElement('form');
      const fieldset = document.createElement('fieldset');
      appendText(fieldset, 'legend', 'Grant to issue');
      const choices = [];
      for (const capability of ['vapor_deploy', 'vapor_service_control', 'host_admin']) {
        const label = document.createElement('label');
        const input = document.createElement('input');
        input.type = 'checkbox'; input.value = capability;
        input.checked = (item.requested_capabilities || []).includes(capability);
        choices.push(input);
        label.append(input, document.createTextNode(` ${capabilityLabel(capability)}${capability === 'host_admin' ? ' (temporary account, not direct root SSH)' : ''}`));
        fieldset.append(label);
      }
      const durationLabel = document.createElement('label'); durationLabel.textContent = 'Grant duration (hours) ';
      const duration = document.createElement('input'); duration.type = 'number'; duration.min = '0.25'; duration.max = '8'; duration.step = '0.25';
      duration.value = String(Math.max(0.25, Math.min((Number(item.requested_duration_seconds) || 3600) / 3600, 8)));
      duration.required = true; durationLabel.append(duration);
      const reviewLabel = document.createElement('label'); const review = document.createElement('input'); review.type = 'checkbox'; review.required = false;
      reviewLabel.append(review, document.createTextNode(' I reviewed the key, purpose, and actual grant'));
      const hostLabel = document.createElement('label'); const hostConfirm = document.createElement('input'); hostConfirm.type = 'checkbox';
      hostLabel.append(hostConfirm, document.createTextNode(' I approve a temporary account with unrestricted sudo (not direct root SSH)'));
      const approve = document.createElement('button'); approve.type = 'submit'; approve.textContent = 'Approve selected access';
      form.append(fieldset, durationLabel, reviewLabel, hostLabel, approve);
      form.addEventListener('submit', event => {
        event.preventDefault();
        if (!review.checked) {
          feedback.textContent = 'Confirm the review before approving this SSH request.';
          review.focus();
          return;
        }
        if (!duration.reportValidity()) return;
        const granted = choices.filter(input => input.checked).map(input => input.value);
        if (!granted.length) {
          feedback.textContent = 'Select at least one capability for this temporary grant.';
          choices[0]?.focus();
          return;
        }
        if (granted.includes('host_admin') && (granted.length !== 1 || !hostConfirm.checked)) {
          feedback.textContent = 'Full temporary sudo requires its separate confirmation and cannot be combined with other capabilities.';
          return;
        }
        submitDecision(approve, 'ssh', item, 'approve', { expected_version: item.version, confirmed: true,
          host_admin_confirmed: hostConfirm.checked, granted_capabilities: granted,
          granted_duration_seconds: Number(duration.value) * 3600 });
      });
      card.append(form);
      card.append(confirmationForm(item, 'I intend to reject this access request', 'Reject', button => submitDecision(button, 'ssh', item, 'reject', { expected_version: item.version, confirmed: true })));
    };
    const renderIdentity = (item, card) => {
      appendText(card, 'h3', `${item.action || 'permission change'} · ${item.role || 'unknown role'}`);
      appendText(card, 'p', `Steam ${item.steam_id64 || 'not linked'} · GitHub ${item.github_login || 'not linked'}`);
      const metadata = document.createElement('p'); metadata.className = 'muted';
      metadata.textContent = `Submitted by ${item.requester_kind || 'unknown'} · profile ${item.requester_profile_id || 'unknown'}`;
      card.append(requestMetadata(metadata));
      appendText(card, 'p', `Expires ${humanTime(item.expires_at_unix)}`, 'muted');
      card.append(confirmationForm(item, `I reviewed this ${item.action || 'permission'} for ${item.github_login || item.steam_id64 || 'this account'}`, 'Approve', button => submitDecision(button, 'identity', item, 'approve', { expected_version: item.version, confirmed: true })));
      const form = document.createElement('form');
      const reason = document.createElement('textarea'); reason.required = true; reason.maxLength = 500; reason.rows = 2;
      reason.setAttribute('aria-label', 'Reason for rejecting this Identity request'); reason.placeholder = 'Reason for rejection';
      const label = document.createElement('label'); const confirm = document.createElement('input'); confirm.type = 'checkbox'; requireConfirmation(confirm);
      label.append(confirm, document.createTextNode(' I intend to reject this permission request'));
      const reject = document.createElement('button'); reject.type = 'submit'; reject.textContent = 'Reject';
      form.append(reason, label, reject);
      form.addEventListener('submit', event => {
        event.preventDefault();
        if (confirm.checked && reason.value.trim()) submitDecision(reject, 'identity', item, 'reject', { expected_version: item.version, confirmed: true, reason: reason.value.trim() });
      });
      card.append(form);
    };
    const renderExpiredIdentity = (item, card) => {
      appendText(card, 'h3', `${item.action || 'permission change'} · ${item.role || 'unknown role'}`);
      appendText(card, 'p', `Steam ${item.steam_id64 || 'not linked'} · GitHub ${item.github_login || 'not linked'}`);
      const metadata = document.createElement('p'); metadata.className = 'muted';
      metadata.textContent = `Submitted by ${item.requester_kind || 'unknown'} · profile ${item.requester_profile_id || 'unknown'}`;
      card.append(requestMetadata(metadata));
      appendText(card, 'p', `Expired at ${humanTime(item.expires_at_unix)} · Identity has not yet recorded the transition`, 'approval-state-warning');
    };
    const renderSource = source => {
      const container = containers[source.key];
      const state = sourceState[source.key];
      const records = state.records;
      const pending = records.filter(item => item && item.status === 'pending');
      const supersededDeployments = source.key === 'deploy' ? pending.filter(deployIsSuperseded) : [];
      const reviewablePending = source.key === 'deploy' ? pending.filter(item => !deployIsSuperseded(item)) : pending;
      if (countBadges[source.key]) countBadges[source.key].textContent = source.key === 'deploy'
        ? `${reviewablePending.length} to review${supersededDeployments.length ? ` · ${supersededDeployments.length} superseded` : ''}${state.cursor ? ' · more available' : ''}`
        : state.cursor ? `${pending.length} pending · more available` : `${pending.length} pending`;
      const expiredIdentity = source.key === 'identity' ? pending.filter(isExpiredByTime) : [];
      const actionable = source.key === 'identity'
        ? reviewablePending.filter(item => !isExpiredByTime(item))
        : source.key === 'deploy'
          ? reviewablePending.slice().sort((a, b) => Number(a.created_at_unix || 0) - Number(b.created_at_unix || 0) || String(a.id).localeCompare(String(b.id)))
          : reviewablePending;
      container.replaceChildren();
      if (!actionable.length && !supersededDeployments.length) appendText(container, 'p', 'No requests need a decision in the loaded records.', 'muted');
      else if (!actionable.length && source.key === 'deploy') appendText(container, 'p', 'No deployment approvals need a decision.', 'muted');
      for (const item of actionable) {
        const card = document.createElement('article'); card.className = 'card';
        if (source.key === 'deploy') renderDeploy(item, card);
        else if (source.key === 'ssh') renderSsh(item, card);
        else renderIdentity(item, card);
        container.append(card);
      }
      for (const item of expiredIdentity) {
        const card = document.createElement('article'); card.className = 'card';
        renderExpiredIdentity(item, card);
        container.append(card);
      }
      if (supersededDeployments.length) {
        const stale = document.createElement('details');
        const staleSummary = document.createElement('summary');
        staleSummary.textContent = `${supersededDeployments.length} superseded deployment request${supersededDeployments.length === 1 ? '' : 's'}`;
        stale.append(staleSummary);
        appendText(stale, 'p', 'These requests target an older main revision. They cannot be approved and do not block the current request.', 'muted');
        for (const item of supersededDeployments) {
          const card = document.createElement('article'); card.className = 'card';
          renderDeploy(item, card);
          stale.append(card);
        }
        container.append(stale);
      }
      if (pageButtons[source.key]) {
        pageButtons[source.key].hidden = !state.cursor;
        pageButtons[source.key].disabled = false;
        pageButtons[source.key].textContent = 'Load older requests';
      }
    };
    const readSourcePage = async (source, cursor, generation) => {
      const state = sourceState[source.key];
      const url = new URL(source.url, window.location.href);
      if (cursor) {
        url.searchParams.set('limit', '200');
        url.searchParams.set('cursor', cursor);
      }
      const response = await fetch(url, { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(6000) });
      const body = await response.json().catch(() => null);
      if (state.generation !== generation) return false;
      if (!response.ok) {
        state.error = response.status === 401 ? 'Sign in required.' : response.status === 403 ? 'Root access required.' : `Unavailable (HTTP ${response.status}).`;
        throw new Error(state.error);
      }
      const items = Array.isArray(body) ? body : body?.items;
      if (!Array.isArray(items)) throw new Error('invalid response');
      state.records.push(...items);
      state.cursor = Array.isArray(body) ? null : (typeof body.next_cursor === 'string' ? body.next_cursor : null);
      state.error = null;
      return true;
    };
    const refreshSource = async source => {
      const container = containers[source.key];
      const state = sourceState[source.key];
      const generation = ++state.generation;
      state.records = [];
      state.cursor = null;
      container.textContent = 'Loading…';
      try {
        await readSourcePage(source, null, generation);
        if (state.generation !== generation) return;
        renderSource(source);
      } catch (_) {
        if (state.generation !== generation) return;
        container.replaceChildren();
        if (countBadges[source.key]) countBadges[source.key].textContent = 'Unavailable';
        appendText(container, 'p', 'This request source is temporarily unavailable or returned an invalid response.', 'error');
      }
    };
    const loadMoreSource = async source => {
      const button = pageButtons[source.key];
      const state = sourceState[source.key];
      if (!button || button.disabled || !state.cursor) return;
      const generation = state.generation;
      button.disabled = true;
      button.textContent = 'Loading…';
      try {
        await readSourcePage(source, state.cursor, generation);
        if (state.generation !== generation) return;
        renderSource(source);
      } catch (_) {
        if (state.generation !== generation) return;
        button.disabled = false;
        button.textContent = 'Retry loading older requests';
        feedback.textContent = `${source.label} could not load another page. Existing requests remain visible.`;
      }
    };
    const refreshInbox = async () => {
      if (refreshButton?.disabled) return;
      if (refreshButton) { refreshButton.disabled = true; refreshButton.textContent = 'Refreshing…'; }
      await Promise.all(sourceDefinitions.map(refreshSource));
      if (refreshButton) { refreshButton.disabled = false; refreshButton.textContent = 'Refresh'; }
    };
    refreshButton?.addEventListener('click', refreshInbox);
    pageButtons.deploy?.addEventListener('click', () => loadMoreSource(sourceDefinitions[0]));
    pageButtons.ssh?.addEventListener('click', () => loadMoreSource(sourceDefinitions[1]));
    pageButtons.identity?.addEventListener('click', () => loadMoreSource(sourceDefinitions[2]));
    refreshInbox();
  }

  const operationHistory = document.querySelector('#operations-history');
  if (operationHistory) {
    const search = document.querySelector('#history-search');
    const kindFilter = document.querySelector('#history-kind');
    const statusFilter = document.querySelector('#history-status');
    const count = document.querySelector('#history-count');
    const sourceStatus = document.querySelector('#history-sources');
    const sources = [
      { kind: 'Platform operation', label: 'Platform deploy/restore operations', url: '/api/admin/operations/history' },
      { kind: 'Deployment', label: 'Platform deployments', url: '/api/admin/deploy-requests' },
      { kind: 'Temporary access', label: 'Platform temporary access', url: '/api/admin/ssh-access/requests' },
      { kind: 'Identity request', label: 'Identity permission requests', url: '/api/identity/v1/admin/permission-requests/page' }
    ];
    const validTimestamp = value => Number.isFinite(value) && value > 0 && value <= 8640000000000;
    const humanTime = value => validTimestamp(value)
      ? new Date(value * 1000).toLocaleString() : 'Time not recorded';
    const isoTime = value => validTimestamp(value) ? new Date(value * 1000).toISOString() : '';
    const safeText = value => typeof value === 'string' ? value : '';
    let records = [];
    let selectedRecordId = null;
    let failures = [];
    let sourceStates = [];
    let pendingRequestsOmitted = 0;
    const isOpenHistoryRecord = (source, item) =>
      source.kind !== 'Platform operation' && item.status === 'pending';
    const addRecord = (source, item) => {
      if (!item || typeof item !== 'object') return;
      if (isOpenHistoryRecord(source, item)) { pendingRequestsOmitted += 1; return; }
      const isOperation = source.kind === 'Platform operation';
      const record = { kind: source.kind, source: source.label, id: safeText(item.id || item.operation_id), status: safeText(isOperation ? item.result : item.status), statusKey: safeText(isOperation ? item.result : item.status), dispatchStatusKey: '', statusLabel: safeText(isOperation ? item.result : item.status), timestamp: Number(item.finished_at_unix || item.decided_at_unix || item.created_at_unix || 0), timestampLabel: item.finished_at_unix ? 'Operation finished' : item.decided_at_unix ? 'Decision recorded' : 'Request created', title: '', subject: '', details: [], link: '', search: '' };
      if (isOperation) {
        record.title = `${safeText(item.operation)} · ${safeText(item.result)}`;
        record.subject = `Revision ${safeText(item.revision)}`;
        record.details = [
          `Started: ${humanTime(Number(item.started_at_unix))}`,
          `Finished: ${humanTime(Number(item.finished_at_unix))}`,
          `Mode: ${safeText(item.server_mode)}`,
          `Operation ID: ${safeText(item.operation_id)}`
        ];
        record.search = [record.title, record.subject, ...record.details].join(' ');
      } else if (source.kind === 'Deployment') {
        record.dispatchStatusKey = `dispatch-${safeText(item.dispatch_state)}`;
        record.statusLabel = `${safeText(item.status)} · dispatch ${safeText(item.dispatch_state) || 'not reported'}`;
        if (item.decided_at_unix) record.timestampLabel = 'Decision recorded';
        record.title = `${safeText(item.repository)} · ${safeText(item.branch)}`;
        record.subject = `Commit ${safeText(item.commit)}`;
        record.details = [
          `Submitted by: ${safeText(item.requester_kind) || 'unknown'} · profile ${safeText(item.requester_profile_id) || 'unknown'}`,
          item.expires_at_unix ? `Request expires: ${humanTime(Number(item.expires_at_unix))}` : '',
          `Decision: ${item.status === 'pending' ? 'awaiting review' : safeText(item.status)}${item.decision_reason ? ` · ${safeText(item.decision_reason)}` : ''}`,
          `Dispatch: ${safeText(item.dispatch_state) || 'not reported'}${item.dispatch_attempts ? ` · ${item.dispatch_attempts} attempt(s)` : ''}${item.dispatch_detail ? ` · ${safeText(item.dispatch_detail)}` : ''}`,
          item.result_detail ? `Outcome: ${safeText(item.result_detail)}` : '',
          item.summary ? `Request: ${safeText(item.summary)}` : ''
        ].filter(Boolean);
        record.link = safeText(item.workflow_url);
        record.search = [record.title, record.subject, item.requester_profile_id, item.requester_kind, item.summary, item.decision_reason, item.dispatch_detail, item.result_detail].join(' ');
      } else if (source.kind === 'Temporary access') {
        record.timestampLabel = 'Request created';
        record.title = safeText(item.purpose) || 'Temporary SSH access request';
        record.subject = `Key ${safeText(item.fingerprint)}`;
        record.details = [
          `Submitted by: ${safeText(item.requester_kind) || 'unknown'} · profile ${safeText(item.requester_profile_id) || 'unknown'}`,
          item.expires_at_unix ? `Grant expires: ${humanTime(Number(item.expires_at_unix))}` : '',
          `Requested capabilities: ${(Array.isArray(item.requested_capabilities) ? item.requested_capabilities : []).join(', ') || 'none'}`,
          `Requested duration: ${item.requested_duration_seconds || 0} seconds`,
          item.granted_duration_seconds != null ? `Granted: ${(Array.isArray(item.granted_capabilities) ? item.granted_capabilities : []).join(', ') || 'no capabilities'} · ${item.granted_duration_seconds} seconds` : '',
          item.detail ? `Latest operation: ${safeText(item.detail)}` : ''
        ].filter(Boolean);
        record.search = [record.title, record.subject, item.requester_profile_id, ...record.details].join(' ');
        record.eventsUrl = `/api/admin/ssh-access/requests/${encodeURIComponent(record.id)}/events`;
      } else {
        const expiredPending = item.status === 'pending' && Number(item.expires_at_unix) > 0 && Number(item.expires_at_unix) <= Math.floor(Date.now() / 1000);
        if (expiredPending) {
          record.statusKey = 'expired-pending';
          record.statusLabel = 'expired · owner transition pending';
        }
        record.title = `${safeText(item.action)} ${safeText(item.role)}`.trim() || 'Identity permission request';
        record.subject = `Steam ${safeText(item.steam_id64)} · GitHub ${safeText(item.github_login)}`;
        record.details = [
          `Submitted by: ${safeText(item.requester_kind) || 'unknown'} · profile ${safeText(item.requester_profile_id) || 'unknown'}`,
          item.expires_at_unix ? `Request expires: ${humanTime(Number(item.expires_at_unix))}` : '',
          expiredPending ? 'The request passed its expiry time. Identity has not yet persisted the expired status; this dashboard does not change owner state.' : '',
          item.decision_reason ? `Decision reason: ${safeText(item.decision_reason)}` : '',
          item.decided_at_unix ? `Decided: ${humanTime(Number(item.decided_at_unix))}` : 'No decision recorded'
        ].filter(Boolean);
        record.link = '/admin/identity';
        record.search = [record.title, record.subject, item.requester_profile_id, item.requester_kind, item.decision_reason].join(' ');
      }
      if (record.id && record.status) records.push(record);
    };
    const historyStatus = record => {
      const status = record.statusKey;
      const dispatch = record.dispatchStatusKey;
      if (['failed', 'rejected', 'expired', 'cancelled', 'revoked', 'expired-pending'].includes(status) || ['dispatch-failed', 'dispatch-unknown'].includes(dispatch)) return 'attention';
      if (record.kind === 'Identity request' && status === 'approved') return 'complete';
      if (record.kind === 'Temporary access' && status === 'active') return 'active';
      if (['pending', 'approved', 'executing', 'provisioning', 'stale'].includes(status) || ['dispatch-dispatching', 'dispatch-queued', 'dispatch-claimed'].includes(dispatch)) return 'active';
      return ['succeeded', 'success', 'active'].includes(status) || dispatch === 'dispatch-completed' ? 'complete' : 'other';
    };
    const decorateHistoryState = (element, record) => {
      element.className = 'history-state';
      if (historyStatus(record) === 'attention') element.classList.add('state-danger');
      else if (historyStatus(record) === 'complete') element.classList.add('state-good');
      else if (historyStatus(record) === 'active') element.classList.add('state-warning');
    };
    const renderHistory = () => {
      const query = (search?.value || '').trim().toLocaleLowerCase();
      const selectedKind = kindFilter?.value || 'all';
      const selectedStatus = statusFilter?.value || 'all';
      const visible = records.filter(record =>
        (selectedKind === 'all' || record.kind === selectedKind) &&
        (selectedStatus === 'all' || historyStatus(record) === selectedStatus) &&
        (!query || `${record.kind} ${record.statusLabel} ${record.id} ${record.search}`.toLocaleLowerCase().includes(query))
      );
      operationHistory.replaceChildren();
      count.textContent = `Showing ${visible.length} of ${records.length} loaded records.`;
      if (!records.length && failures.length === sources.length) {
        operationHistory.textContent = 'History could not be loaded from any owner service.';
        return;
      }
      if (!visible.length) {
        if (records.length) {
          operationHistory.textContent = 'No records match these filters.';
        } else if (pendingRequestsOmitted) {
          const message = document.createElement('p');
          message.append(document.createTextNode('No resolved records in the loaded pages. Pending requests are in '));
          const link = document.createElement('a'); link.href = '/admin/approvals'; link.textContent = 'Approvals';
          message.append(link, document.createTextNode('.'));
          operationHistory.append(message);
        } else {
          operationHistory.textContent = 'No records have been recorded by the available services.';
        }
        return;
      }
      const selected = visible.find(record => record.id === selectedRecordId) || visible[0];
      selectedRecordId = selected.id;
      const workbench = document.createElement('div'); workbench.className = 'history-workbench';
      const list = document.createElement('ul'); list.className = 'history-record-list'; list.setAttribute('aria-label', 'History records');
      for (const record of visible) {
        const item = document.createElement('li');
        const button = document.createElement('button'); button.type = 'button'; button.className = 'history-record';
        button.setAttribute('aria-current', String(record.id === selected.id));
        button.addEventListener('click', () => { selectedRecordId = record.id; renderHistory(); });
        const title = document.createElement('span'); title.className = 'history-record-title'; title.textContent = record.title;
        const meta = document.createElement('span'); meta.className = 'history-record-meta';
        const kind = document.createElement('span'); kind.textContent = record.kind;
        const date = document.createElement('time'); date.dateTime = isoTime(record.timestamp); date.textContent = humanTime(record.timestamp);
        const state = document.createElement('span'); state.textContent = record.statusLabel; decorateHistoryState(state, record);
        meta.append(kind, date, state); button.append(title, meta); item.append(button); list.append(item);
      }
      const detail = document.createElement('section'); detail.className = 'history-record-detail'; detail.setAttribute('aria-live', 'polite');
      const header = document.createElement('header');
      const heading = document.createElement('h2'); heading.textContent = selected.title;
      const state = document.createElement('span'); state.textContent = selected.statusLabel; decorateHistoryState(state, selected);
      header.append(heading, state); detail.append(header);
      const meta = document.createElement('p'); meta.className = 'history-meta';
      const kind = document.createElement('span'); kind.className = 'history-kind'; kind.textContent = selected.kind;
      const date = document.createElement('time'); date.dateTime = isoTime(selected.timestamp); date.textContent = `${selected.timestampLabel}: ${humanTime(selected.timestamp)}`;
      const id = document.createElement('span'); id.textContent = `${selected.kind === 'Platform operation' ? 'Operation' : 'Request'} ${selected.id}`;
      meta.append(kind, date, id); detail.append(meta);
      if (selected.subject) { const subject = document.createElement('p'); subject.className = 'history-detail'; subject.textContent = selected.subject; detail.append(subject); }
      if (selected.details.length) {
        const details = document.createElement('ul'); details.className = 'history-detail-list';
        for (const value of selected.details) { const line = document.createElement('li'); line.textContent = value; details.append(line); }
        detail.append(details);
      }
      if (selected.link) { const link = document.createElement('a'); link.href = selected.link; link.textContent = selected.kind === 'Deployment' ? 'Open CI run' : 'Open owning section'; link.rel = 'noopener noreferrer'; detail.append(link); }
      if (selected.eventsUrl) {
        const events = document.createElement('details');
        const summary = document.createElement('summary'); summary.textContent = 'Request activity';
        const body = document.createElement('div'); body.className = 'history-detail'; body.textContent = 'Open to load owner-recorded status changes.';
        events.append(summary, body);
        events.addEventListener('toggle', async () => {
          if (!events.open || events.dataset.loaded === 'true') return;
          body.textContent = 'Loading activity…';
          try {
            const response = await fetch(selected.eventsUrl, { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(6000) });
            const transitions = await response.json().catch(() => null);
            if (!response.ok || !Array.isArray(transitions)) { body.textContent = response.status === 403 ? 'Root access required.' : `Activity unavailable (HTTP ${response.status}).`; return; }
            body.replaceChildren();
            if (!transitions.length) body.textContent = 'No request transitions were recorded.';
            for (const event of transitions) {
              const line = document.createElement('p'); line.className = 'history-detail';
              const transition = event.from_status ? `${event.from_status} → ${event.to_status}` : event.to_status;
              line.textContent = `${humanTime(Number(event.at_unix))} · ${transition} · ${safeText(event.actor_label || event.actor_kind)}`;
              body.append(line);
            }
            events.dataset.loaded = 'true';
          } catch (_) { body.textContent = 'Could not contact the Platform history API.'; }
        });
        detail.append(events);
      }
      workbench.append(list, detail); operationHistory.append(workbench);
    };
    const refreshButton = document.querySelector('#history-refresh');
    const loadMoreButton = document.querySelector('#history-load-more');
    const renderSourceStatus = () => {
      sourceStatus.replaceChildren();
      for (const state of sourceStates) {
        const line = document.createElement('span');
        if (state.error) line.className = 'source-error';
        const summary = state.error || `${state.loaded} loaded${state.nextCursor ? ' · more available' : ''}`;
        line.textContent = `${state.source.label}: ${summary}`;
        sourceStatus.append(line);
      }
      const refreshed = document.createElement('span');
      refreshed.textContent = `Updated ${new Date().toLocaleTimeString()}`;
      sourceStatus.append(refreshed);
      if (loadMoreButton) {
        loadMoreButton.hidden = !sourceStates.some(state => state.nextCursor);
        loadMoreButton.disabled = false;
        loadMoreButton.textContent = 'Load older records';
      }
    };
    const loadMoreHistory = async () => {
      if (loadMoreButton?.disabled) return;
      const pending = sourceStates.filter(state => state.nextCursor);
      if (!pending.length) return;
      if (loadMoreButton) { loadMoreButton.disabled = true; loadMoreButton.textContent = 'Loading…'; }
      await Promise.all(pending.map(async state => {
        try {
          const params = new URLSearchParams({ limit: '50', cursor: state.nextCursor });
          const response = await fetch(`${state.source.url.split('?')[0]}?${params}`, { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(6000) });
          const body = await response.json().catch(() => null);
          if (!response.ok) throw new Error(response.status === 403 ? 'Root access required.' : `Unavailable (HTTP ${response.status}).`);
          const items = Array.isArray(body) ? body : body?.items;
          if (!Array.isArray(items)) throw new Error('Invalid owner response.');
          for (const item of items) addRecord(state.source, item);
          state.loaded += items.length;
          state.nextCursor = Array.isArray(body) ? null : (typeof body.next_cursor === 'string' ? body.next_cursor : null);
          state.error = null;
        } catch (error) { state.error = error.message || 'Could not contact the owner service.'; }
      }));
      records.sort((a, b) => b.timestamp - a.timestamp || a.id.localeCompare(b.id));
      renderSourceStatus();
      renderHistory();
    };
    const loadHistory = async () => {
      if (refreshButton?.disabled) return;
      if (refreshButton) { refreshButton.disabled = true; refreshButton.textContent = 'Refreshing…'; }
      records = [];
      failures = [];
      pendingRequestsOmitted = 0;
      operationHistory.replaceChildren();
      sourceStatus.textContent = 'Loading recent request records from their owning services…';
      const results = await Promise.all(sources.map(async source => {
        try {
          const url = `${source.url}?limit=50`;
          const response = await fetch(url, { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(6000) });
          const body = await response.json().catch(() => null);
          if (!response.ok) return { source, error: response.status === 401 ? 'Sign in required.' : response.status === 403 ? 'Root access required.' : `Unavailable (HTTP ${response.status}).` };
          const items = Array.isArray(body) ? body : body?.items;
          if (!Array.isArray(items)) return { source, error: 'The service returned an invalid history response.' };
          return { source, items, nextCursor: Array.isArray(body) ? null : (typeof body.next_cursor === 'string' ? body.next_cursor : null) };
        } catch (_) { return { source, error: 'Could not contact this service.' }; }
      }));
      sourceStates = results.map(result => ({ source: result.source, loaded: result.items?.length || 0, nextCursor: result.nextCursor || null, error: result.error || null }));
      for (const result of results) {
        if (result.error) failures.push(`${result.source.label}: ${result.error}`);
        else for (const item of result.items) addRecord(result.source, item);
      }
      records.sort((a, b) => b.timestamp - a.timestamp || a.id.localeCompare(b.id));
      renderSourceStatus();
      renderHistory();
      if (refreshButton) { refreshButton.disabled = false; refreshButton.textContent = 'Refresh records'; }
    };
    search?.addEventListener('input', renderHistory);
    kindFilter?.addEventListener('change', renderHistory);
    statusFilter?.addEventListener('change', renderHistory);
    refreshButton?.addEventListener('click', loadHistory);
    loadMoreButton?.addEventListener('click', loadMoreHistory);
    loadHistory();
  }

  const deployRequests = document.querySelector('#deploy-requests');
  if (deployRequests) {
    const isQueuedOrRunning = item => item.status === 'executing' ||
      (item.status === 'approved' && ['queued', 'dispatching'].includes(item.dispatch_state));
    const isDispatchUncertain = item => ['approved', 'failed'].includes(item.status) && item.dispatch_state === 'unknown';
    const isFailedDispatch = item => item.status === 'approved' && item.dispatch_state === 'failed';
    const actionFeedback = new Map();
    const refreshButton = document.querySelector('#deployment-refresh');
    const moreButton = document.querySelector('#deployments-more');
    let deploymentCursor = null;
    let deploymentItems = [];
    const refreshDeployRequests = async () => {
      if (moreButton?.disabled) return;
      if (refreshButton) { refreshButton.disabled = true; refreshButton.textContent = 'Refreshing…'; }
      if (moreButton) moreButton.disabled = true;
      let response;
      let requests;
      try {
        response = await fetch('/api/admin/deploy-requests?limit=200', { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(6000) });
        requests = await response.json().catch(() => null);
      } catch (_) {
        deployRequests.textContent = 'Deploy approvals are temporarily unavailable.';
        if (refreshButton) { refreshButton.disabled = false; refreshButton.textContent = 'Refresh'; }
        if (moreButton) { moreButton.disabled = false; moreButton.hidden = !deploymentCursor; }
        return;
      }
      deployRequests.replaceChildren();
      if (!response.ok) {
        deployRequests.textContent = response.status === 401 ? 'Sign in to review deploy requests.' :
          response.status === 403 ? 'Root access is required to review deploy requests.' :
          'Deploy approvals are temporarily unavailable.';
        if (refreshButton) { refreshButton.disabled = false; refreshButton.textContent = 'Refresh'; }
        if (moreButton) { moreButton.disabled = false; moreButton.hidden = !deploymentCursor; }
        return;
      }
      deploymentItems = Array.isArray(requests) ? requests : (Array.isArray(requests?.items) ? requests.items : []);
      deploymentCursor = Array.isArray(requests) ? null : (typeof requests?.next_cursor === 'string' ? requests.next_cursor : null);
      renderDeployments();
      if (refreshButton) { refreshButton.disabled = false; refreshButton.textContent = 'Refresh'; }
    };
    const renderDeployments = () => {
      deployRequests.replaceChildren();
      if (moreButton) {
        moreButton.hidden = !deploymentCursor;
        moreButton.disabled = false;
        moreButton.textContent = 'Load older records';
      }
      const items = deploymentItems.filter(item => item && (isQueuedOrRunning(item) || isDispatchUncertain(item) || isFailedDispatch(item)));
      if (!items.length) {
        const approvalsLink = document.createElement('a'); approvalsLink.href = '/admin/approvals#pending-deploy-approvals';
        approvalsLink.textContent = 'Approvals';
        const historyLink = document.createElement('a'); historyLink.href = '/admin/history';
        historyLink.textContent = 'History';
        deployRequests.replaceChildren(
          document.createTextNode(deploymentCursor ? 'No queued, running, uncertain, or failed-dispatch records in the loaded page. Older records may remain. Pending approvals are in ' : 'No queued, running, uncertain, or failed-dispatch records. Pending approvals are in '),
          approvalsLink,
          document.createTextNode('; completed and closed runs are in '),
          historyLink,
          document.createTextNode('.')
        );
        return;
      }
      const queuedOrRunningItems = items.filter(isQueuedOrRunning);
      const uncertainItems = items.filter(isDispatchUncertain);
      const failedItems = items.filter(isFailedDispatch);
      const groups = [
        ['Queued or running', queuedOrRunningItems],
        ['Dispatch outcome uncertain', uncertainItems],
        ['Dispatch failed before the worker started', failedItems]
      ];
      for (const [groupTitle, groupItems] of groups) {
        if (!groupItems.length) continue;
        appendText(deployRequests, 'h3', `${groupTitle} · ${groupItems.length}`);
        for (const item of groupItems) {
        const card = document.createElement('article');
        card.className = 'card deployment-card';
        const header = document.createElement('header');
        header.className = 'deployment-card-header';
        const heading = document.createElement('h3');
        heading.textContent = `${item.repository || 'Unknown repository'} · ${item.branch || 'unknown branch'}`;
        const status = document.createElement('span');
        const knownStatuses = new Set(['approved', 'executing', 'succeeded', 'failed', 'rejected', 'expired']);
        status.className = `status-pill deployment-status${knownStatuses.has(item.status) ? ` status-${item.status}` : ''}`;
        status.textContent = item.status;
        header.append(heading, status);
        const summary = document.createElement('p');
        summary.textContent = item.summary || 'No request summary provided.';
        const target = document.createElement('p');
        target.className = validDeploymentSelection(item) ? 'muted' : 'error';
        target.textContent = `Target: ${deploymentSelectionLabel(item)}`;
        const retiredDispatch = item.status === 'failed' && item.dispatch_state === 'failed' && String(item.decision_reason || '').startsWith('Closed after failed dispatch:');
        const currentMain = typeof item.current_main_commit === 'string' ? item.current_main_commit.toLowerCase() : null;
        const supersededBy = currentMain && String(item.commit || '').toLowerCase() !== currentMain ? currentMain : null;
        const dispatchLabels = {
          not_attempted: 'Not dispatched', dispatching: 'Dispatch in progress',
          queued: 'Queued for worker', claimed: 'Worker claimed deployment',
          completed: 'Worker finished deployment', expired: 'Expired before worker claim',
          failed: 'Dispatch or worker claim failed', unknown: 'Dispatch result unknown'
        };
        const lifecycleLabel = item.status === 'pending' ? 'Awaiting approval' :
          item.status === 'executing' ? 'Worker claimed; deployment running' :
          item.status === 'succeeded' ? 'Deployment succeeded' :
          item.status === 'failed' && item.dispatch_state === 'unknown' ? 'Worker stopped; outcome unknown' :
          retiredDispatch ? 'Closed before deployment' :
          supersededBy && item.status === 'approved' && item.dispatch_state === 'failed' ? 'Superseded · dispatch failed before worker started' :
          item.status === 'failed' && item.dispatch_state === 'failed' ? 'Dispatch failed before worker claim' :
          item.status === 'failed' ? 'Deployment failed' :
          item.status === 'expired' ? 'Expired before worker claim' : null;
        const stage = document.createElement('p');
        stage.className = supersededBy ? 'approval-state-warning' : 'muted';
        const canRetry = validDeploymentSelection(item) && item.status === 'approved'
          && item.retry_eligible && ['failed', 'unknown'].includes(item.dispatch_state);
        stage.textContent = `${lifecycleLabel || dispatchLabels[item.dispatch_state] || 'Stage not reported'}${canRetry ? ' · retry available' : ''}`;
        const requester = document.createElement('p');
        requester.className = 'muted';
        requester.textContent = `Submitted by ${item.requester_kind || 'unknown'} · profile ${item.requester_profile_id || 'unknown'}`;
        const revision = document.createElement('p');
        revision.textContent = `Commit ${item.commit} · tree ${item.tree}`;
        const pins = document.createElement('p');
        pins.className = 'muted';
        const expiresAt = Number.isFinite(item.expires_at_unix)
          ? new Date(item.expires_at_unix * 1000).toLocaleString()
          : 'unknown';
        pins.textContent = `${Array.isArray(item.submodules) ? item.submodules.length : 0} pinned service revisions · expires ${expiresAt}`;
        const workflow = document.createElement('a');
        workflow.rel = 'noopener noreferrer';
        workflow.textContent = 'Open successful CI run';
        card.append(header, summary, target, stage);
        if (actionFeedback.has(item.id)) {
          const feedback = document.createElement('p');
          feedback.className = item.status === 'approved' && ['failed', 'unknown'].includes(item.dispatch_state) ? 'error' : 'muted';
          feedback.textContent = actionFeedback.get(item.id);
          card.append(feedback);
        }
        const outcome = document.createElement('p');
        outcome.className = 'muted';
        outcome.textContent = item.result_detail || '';
        const details = document.createElement('details');
        const summaryLabel = document.createElement('summary');
        summaryLabel.textContent = 'Request details';
        const list = document.createElement('ul');
        for (const pin of (Array.isArray(item.submodules) ? item.submodules : [])) {
          const line = document.createElement('li');
          line.textContent = `${pin.path}: ${pin.commit}`;
          list.append(line);
        }
        details.append(summaryLabel, requester, revision, pins);
        if (supersededBy) {
          const stale = document.createElement('p'); stale.className = 'approval-state-warning';
          stale.textContent = `Superseded by main ${supersededBy.slice(0, 12)} · this dispatch cannot be retried.`;
          details.append(stale);
        }
        try {
          const ciUrl = new URL(item.workflow_url);
          if (ciUrl.protocol === 'https:' && ciUrl.hostname === 'github.com' && ciUrl.pathname.startsWith('/GHF-Studios/')) {
            workflow.href = ciUrl.href;
            details.append(workflow);
          }
        } catch (_) {}
        if (item.dispatch_detail) {
          const dispatchDetail = document.createElement('p');
          dispatchDetail.className = 'muted';
          dispatchDetail.textContent = `Dispatch detail: ${String(item.dispatch_detail).slice(0, 500)}`;
          details.append(dispatchDetail);
        }
        if (item.decision_reason) {
          const decisionReason = document.createElement('p');
          decisionReason.className = 'muted';
          decisionReason.textContent = `Decision reason: ${item.decision_reason}`;
          details.append(decisionReason);
        }
        if (item.result_detail) details.append(outcome);
        if (item.dispatch_attempts) {
          const attempts = document.createElement('p');
          attempts.className = 'muted';
          attempts.textContent = `Dispatch attempts: ${item.dispatch_attempts}`;
          details.append(attempts);
        }
        details.append(list);
        card.append(details);
        const actions = [];
        if (canRetry) actions.push('dispatch');
        if (item.status === 'executing') actions.push('reconcile');
        if (item.status === 'approved' && item.dispatch_state === 'failed') actions.push('retire');
        for (const decision of actions) {
          const form = document.createElement('form');
          form.className = 'inline';
          const label = document.createElement('label');
          const confirmed = document.createElement('input');
          confirmed.type = 'checkbox';
          requireConfirmation(confirmed);
          const wording = decision === 'dispatch' ? 'Queue this already-approved revision now' : decision === 'reconcile' ? 'I confirmed this worker is no longer running; mark this execution failed without retrying it' : 'I confirmed dispatch failed before a worker was queued; close this request without deploying it';
          label.append(confirmed, document.createTextNode(` ${wording}`));
          let reason;
          if (decision === 'retire') {
            reason = document.createElement('input');
            reason.type = 'text';
            reason.required = true;
            reason.maxLength = 500;
            reason.setAttribute('aria-label', 'Reason for closing the failed dispatch');
            reason.placeholder = 'Reason for closing this request';
          }
          const button = document.createElement('button');
          button.type = 'submit';
          button.textContent = decision === 'dispatch' ? 'Retry deploy dispatch' : decision === 'reconcile' ? 'Reconcile stopped worker' : 'Close failed dispatch';
          form.append(label);
          if (reason) form.append(reason);
          form.append(button);
          form.addEventListener('submit', async event => {
            event.preventDefault();
            if (!confirmed.checked) return;
            if (reason && !reason.value.trim()) return;
            button.disabled = true;
            try {
              const result = await fetch(`/api/admin/deploy-requests/${encodeURIComponent(item.id)}/${decision}`, {
                method: 'POST', credentials: 'same-origin', cache: 'no-store',
                headers: { 'content-type': 'application/json' },
                body: JSON.stringify({ expected_version: item.version, confirmed: true, ...(reason ? { reason: reason.value.trim() } : {}) })
              });
              const body = await result.clone().json().catch(() => null);
              const plainText = body ? '' : await result.text().catch(() => '');
              const decisionSaved = body?.error?.decision_saved === true;
              const dispatchState = body?.error?.code === 'dispatch_result_unknown' ? 'dispatch outcome is unknown' :
                body?.error?.code === 'dispatch_failed' ? 'dispatch failed' : 'the next deployment stage did not complete';
              const savedStage = decisionSaved
                ? `Approval saved, but ${dispatchState}: ${body.error.message || 'no further detail was reported.'}${body.error.retry_eligible ? ' A retry is available.' : ''}`
                : '';
              const message = savedStage || body?.error?.message || body?.detail || plainText.trim().slice(0, 240) || (result.ok ? 'Action completed.' : `Action failed (${result.status}).`);
              actionFeedback.set(item.id, String(message).slice(0, 240));
            } catch (_) {
              actionFeedback.set(item.id, 'The response was lost. Reload the request to see whether the decision or dispatch stage was saved.');
            }
            await refreshDeployRequests();
          });
          card.append(form);
        }
        deployRequests.append(card);
        }
      }
      if (moreButton) {
        moreButton.hidden = !deploymentCursor;
        moreButton.disabled = false;
        moreButton.textContent = 'Load older records';
      }
    };
    const loadMoreDeployments = async () => {
      if (!deploymentCursor || moreButton?.disabled) return;
      if (moreButton) { moreButton.disabled = true; moreButton.textContent = 'Loading…'; }
      try {
        const params = new URLSearchParams({ limit: '200', cursor: deploymentCursor });
        const response = await fetch(`/api/admin/deploy-requests?${params}`, { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(6000) });
        const body = await response.json().catch(() => null);
        if (!response.ok || !Array.isArray(body?.items)) throw new Error(response.status === 403 ? 'Root access is required to review deploy requests.' : `Older deployments are unavailable (HTTP ${response.status}).`);
        const knownIds = new Set(deploymentItems.map(item => item?.id));
        deploymentItems.push(...body.items.filter(item => item && !knownIds.has(item.id)));
        deploymentCursor = typeof body.next_cursor === 'string' ? body.next_cursor : null;
        renderDeployments();
      } catch (error) {
        const message = document.createElement('p'); message.className = 'error'; message.textContent = error.message || 'Could not load older deployments.';
        deployRequests.prepend(message);
      }
      if (moreButton) { moreButton.disabled = false; moreButton.textContent = 'Load older records'; }
    };
    refreshButton?.addEventListener('click', refreshDeployRequests);
    moreButton?.addEventListener('click', loadMoreDeployments);
    refreshDeployRequests();
  }

  const sshAccessList = document.querySelector('#ssh-access-list');
  const sshAccessDetail = document.querySelector('#ssh-access-detail');
  if (sshAccessList && sshAccessDetail) {
    const search = document.querySelector('#ssh-access-search');
    const filter = document.querySelector('#ssh-access-filter');
    const count = document.querySelector('#ssh-access-count');
    const feedback = document.createElement('p'); feedback.id = 'ssh-access-feedback';
    feedback.setAttribute('role', 'status'); feedback.setAttribute('aria-live', 'polite');
    sshAccessList.closest('.ssh-workbench')?.before(feedback);
    const refreshButton = document.querySelector('#ssh-access-refresh');
    const moreButton = document.querySelector('#ssh-access-more');
    let records = [];
    let nextCursor = null;
    let selectedId = null;
    const statusLabel = value => ({ active: 'Active', pending: 'Pending review', revoked: 'Revoked', rejected: 'Rejected', expired: 'Expired', cancelled: 'Cancelled', failed: 'Setup failed' })[value] || String(value || 'Unknown').replaceAll('_', ' ');
    const formatTime = seconds => Number.isFinite(Number(seconds)) && Number(seconds) > 0
      ? new Date(Number(seconds) * 1000).toLocaleString() : 'Not set';
    const itemName = item => item.username || item.purpose || item.fingerprint || 'SSH access request';
    const makeFact = (label, value) => {
      const wrapper = document.createElement('div'); wrapper.className = 'ssh-fact';
      const term = document.createElement('dt'); term.textContent = label;
      const description = document.createElement('dd'); description.textContent = value || 'Not reported';
      wrapper.append(term, description); return wrapper;
    };
    const isClosed = item => ['revoked', 'rejected', 'expired', 'cancelled'].includes(item.status);
    const matchesFilter = item => {
      const mode = filter.value;
      if (mode === 'active') return item.status === 'active';
      if (mode === 'pending') return item.status === 'pending';
      if (mode === 'history') return isClosed(item) || item.status === 'failed';
      return true;
    };
    const visibleRecords = () => {
      const needle = search.value.trim().toLocaleLowerCase();
      return records.filter(item => item.status !== 'pending' && matchesFilter(item) && (!needle || [itemName(item), item.purpose,
        item.fingerprint, item.username, item.host, item.requester_kind, item.requester_profile_id,
        ...(item.requested_capabilities || []), ...(item.granted_capabilities || [])]
        .some(value => String(value || '').toLocaleLowerCase().includes(needle))));
    };
    const renderDetail = item => {
      sshAccessDetail.replaceChildren();
      if (!item) {
        const empty = document.createElement('div');
        const title = document.createElement('h2'); title.textContent = records.length ? 'No matching access record' : 'No access records yet';
        const text = document.createElement('p'); text.className = 'muted';
        text.textContent = records.length ? 'Change the search or status filter to see other records.' : 'When a request is submitted, review it in Approvals. Granted access appears here.';
        empty.append(title, text); sshAccessDetail.append(empty); return;
      }
      const header = document.createElement('header'); header.className = 'ssh-detail-header';
      const heading = document.createElement('div');
      const title = document.createElement('h2'); title.textContent = itemName(item);
      const purpose = document.createElement('p'); purpose.className = 'muted'; purpose.textContent = item.purpose || 'No purpose supplied';
      heading.append(title, purpose);
      const badge = document.createElement('span'); badge.className = `status-pill ${item.status === 'active' ? 'status-healthy' : item.status === 'pending' ? '' : 'status-unavailable'}`;
      badge.textContent = statusLabel(item.status); header.append(heading, badge); sshAccessDetail.append(header);

      const facts = document.createElement('dl'); facts.className = 'ssh-facts';
      facts.append(makeFact('Public key fingerprint', item.fingerprint),
        makeFact('Requester', `${item.requester_kind || 'Unknown'} · ${item.requester_profile_id || 'Unknown profile'}`),
        makeFact(item.status === 'active' ? 'Expires' : 'Requested duration', item.status === 'active' ? formatTime(item.expires_at_unix) : formatDuration(item.requested_duration_seconds)),
        makeFact('Requested access', capabilityList(item.requested_capabilities) || 'Shell access'));
      if (item.granted_duration_seconds != null) {
        facts.append(makeFact('Granted duration', formatDuration(item.granted_duration_seconds)),
          makeFact('Granted access', capabilityList(item.granted_capabilities) || 'Shell access'));
      }
      if (item.username && item.host) facts.append(makeFact('Connection', `${item.username}@${item.host}:22`));
      sshAccessDetail.append(facts);
      if (item.detail) { const detail = document.createElement('p'); detail.className = 'muted'; detail.textContent = item.detail; sshAccessDetail.append(detail); }

      if (item.status === 'active' || item.status === 'failed') {
        const action = document.createElement('div'); action.className = 'ssh-action-row';
        const label = document.createElement('label'); const confirm = document.createElement('input');
        confirm.type = 'checkbox'; confirm.setAttribute('aria-label', item.status === 'failed' ? 'Confirm cleanup of partial SSH account' : 'Confirm revoking this SSH grant');
        label.append(confirm, document.createTextNode(item.status === 'failed' ? ' Clean up any partial account' : ' Confirm revoke'));
        const button = document.createElement('button'); button.type = 'button'; button.textContent = item.status === 'failed' ? 'Clean up grant' : 'Revoke access';
        button.addEventListener('click', async () => {
          if (!confirm.checked) return;
          button.disabled = true;
          const result = await fetch(`/api/admin/ssh-access/grants/${encodeURIComponent(item.id)}/revoke`, {
            method: 'POST', credentials: 'same-origin', cache: 'no-store',
            headers: { 'content-type': 'application/json' },
            body: JSON.stringify({ expected_version: item.version, confirmed: true })
          }).catch(() => null);
          if (!result) feedback.textContent = 'No response received. Refresh to verify whether access was revoked.';
          else if (!result.ok) feedback.textContent = await responseFailure(result, 'Could not revoke SSH access');
          else { feedback.textContent = 'Access change saved.'; selectedId = item.id; }
          await refresh();
        });
        action.append(label, button); sshAccessDetail.append(action);
      }

      const history = document.createElement('section'); history.className = 'ssh-history';
      const toggle = document.createElement('button'); toggle.type = 'button'; toggle.setAttribute('aria-expanded', 'false'); toggle.textContent = 'Show request history';
      const historyBody = document.createElement('div'); historyBody.className = 'ssh-history-list'; historyBody.hidden = true;
      let historyLoaded = false;
      toggle.addEventListener('click', async () => {
        const open = historyBody.hidden; historyBody.hidden = !open; toggle.setAttribute('aria-expanded', String(open));
        toggle.textContent = open ? 'Hide request history' : 'Show request history';
        if (!open || historyLoaded) return;
        historyBody.textContent = 'Loading history…';
        const response = await fetch(`/api/admin/ssh-access/requests/${encodeURIComponent(item.id)}/events`, { credentials: 'same-origin', cache: 'no-store' }).catch(() => null);
        if (!response) { historyBody.textContent = 'Could not contact the temporary SSH service.'; return; }
        if (!response.ok) { await showResponseFailure(historyBody, response, 'Could not load request history'); return; }
        const events = await response.json().catch(() => null);
        if (!Array.isArray(events)) { historyBody.textContent = 'The SSH service returned invalid history.'; return; }
        historyBody.replaceChildren();
        if (!events.length) historyBody.textContent = 'No access history recorded.';
        for (const event of events) {
          const line = document.createElement('p');
          const transition = event.from_status ? `${event.from_status} → ${event.to_status}` : event.to_status;
          line.textContent = `${formatTime(event.at_unix)} · ${transition} · ${event.actor_kind || 'actor'} · ${event.actor_label || event.actor_profile_id || 'unknown'}`;
          historyBody.append(line);
        }
        historyLoaded = true;
      });
      history.append(toggle, historyBody); sshAccessDetail.append(history);
    };
    const renderList = () => {
      const visible = visibleRecords();
      const active = records.filter(item => item.status === 'active').length;
      const pending = records.filter(item => item.status === 'pending').length;
      count.textContent = `${visible.length} grants shown · ${active} active · ${pending} awaiting review${nextCursor ? ' in loaded records · more available' : ''}`;
      if (moreButton) moreButton.hidden = !nextCursor;
      sshAccessList.replaceChildren();
      if (!visible.length) {
        const row = document.createElement('li'); row.className = 'muted';
        row.textContent = records.length ? 'No records match this view.' : 'No SSH access records were returned.';
        sshAccessList.append(row); renderDetail(null); return;
      }
      if (!visible.some(item => item.id === selectedId)) selectedId = visible[0].id;
      for (const item of visible) {
        const row = document.createElement('li'); const button = document.createElement('button');
        button.type = 'button'; button.className = 'ssh-record'; button.setAttribute('aria-current', String(item.id === selectedId));
        const name = document.createElement('strong'); name.textContent = itemName(item);
        const state = document.createElement('span'); state.className = 'ssh-record-state'; state.textContent = `${statusLabel(item.status)} · ${item.fingerprint || 'fingerprint unavailable'}`;
        const meta = document.createElement('span'); meta.textContent = item.username && item.host ? `${item.username}@${item.host}` : (item.purpose || 'No purpose supplied');
        button.append(name, state, meta); button.addEventListener('click', () => { selectedId = item.id; renderList(); });
        row.append(button); sshAccessList.append(row);
      }
      renderDetail(visible.find(item => item.id === selectedId));
    };
    const refresh = async () => {
      if (moreButton?.disabled) return;
      if (refreshButton) refreshButton.disabled = true;
      if (moreButton) moreButton.disabled = true;
      let response;
      try { response = await fetch('/api/admin/ssh-access/requests?limit=200', { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(6000) }); }
      catch (_) {
        sshAccessList.innerHTML = '<li class="error">Could not contact the temporary SSH service.</li>';
        if (refreshButton) refreshButton.disabled = false;
        if (moreButton) { moreButton.disabled = false; moreButton.hidden = !nextCursor; }
        return;
      }
      if (!response.ok) { await showResponseFailure(sshAccessList, response, 'Could not load SSH access records'); if (refreshButton) refreshButton.disabled = false; if (moreButton) { moreButton.disabled = false; moreButton.hidden = !nextCursor; } return; }
      const body = await response.json().catch(() => null);
      const updatedRecords = Array.isArray(body) ? body : body?.items;
      if (!Array.isArray(updatedRecords)) { sshAccessList.innerHTML = '<li class="error">The SSH service returned an invalid response.</li>'; if (refreshButton) refreshButton.disabled = false; if (moreButton) { moreButton.disabled = false; moreButton.hidden = !nextCursor; } return; }
      records = updatedRecords;
      nextCursor = Array.isArray(body) ? null : (typeof body?.next_cursor === 'string' ? body.next_cursor : null);
      records.sort((a, b) => (b.created_at_unix || 0) - (a.created_at_unix || 0));
      renderList(); if (refreshButton) refreshButton.disabled = false; if (moreButton) moreButton.disabled = false;
    };
    const loadMore = async () => {
      if (!nextCursor || moreButton?.disabled) return;
      if (moreButton) { moreButton.disabled = true; moreButton.textContent = 'Loading…'; }
      try {
        const params = new URLSearchParams({ limit: '200', cursor: nextCursor });
        const response = await fetch(`/api/admin/ssh-access/requests?${params}`, { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(6000) });
        const body = await response.json().catch(() => null);
        if (!response.ok || !Array.isArray(body?.items)) throw new Error(response.status === 403 ? 'Root access is required to review SSH access.' : `Older access records are unavailable (HTTP ${response.status}).`);
        const knownIds = new Set(records.map(item => item?.id));
        records.push(...body.items.filter(item => item && !knownIds.has(item.id)));
        nextCursor = typeof body.next_cursor === 'string' ? body.next_cursor : null;
        records.sort((a, b) => (b.created_at_unix || 0) - (a.created_at_unix || 0));
        renderList();
      } catch (error) {
        feedback.textContent = error.message || 'Could not load older SSH access records.';
      }
      if (moreButton) { moreButton.disabled = false; moreButton.textContent = 'Load older records'; }
    };
    search.addEventListener('input', renderList); filter.addEventListener('change', renderList);
    refreshButton?.addEventListener('click', refresh); moreButton?.addEventListener('click', loadMore); refresh();
  }

  const recoveryFeedback = document.querySelector('#recovery-feedback');
  const recoveryFacts = document.querySelector('#recovery-facts');
  const recoveryRefresh = document.querySelector('#recovery-refresh');
  const recoverySnapshot = document.querySelector('#recovery-snapshot');
  if (recoveryFeedback && recoveryFacts) {
    const fact = (label, value, state = '') => {
      const wrap = document.createElement('div'); wrap.className = 'recovery-fact';
      const term = document.createElement('dt'); term.textContent = label;
      const detail = document.createElement('dd'); detail.textContent = value;
      if (state) detail.className = `state-${state}`;
      wrap.append(term, detail); return wrap;
    };
    const humanSize = value => {
      let size = Number(value); const units = ['B', 'KB', 'MB', 'GB', 'TB']; let index = 0;
      while (size >= 1024 && index < units.length - 1) { size /= 1024; index++; }
      return `${size.toFixed(index ? 1 : 0)} ${units[index]}`;
    };
    const refreshRecovery = async () => {
      if (recoveryRefresh) recoveryRefresh.disabled = true; recoveryFeedback.textContent = 'Checking recovery evidence…';
      try {
        const response = await fetch('/api/admin/recovery/status', { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(6000) });
        if (!response.ok) throw new Error(response.status === 401 || response.status === 403 ? 'Root sign-in is required.' : `Recovery status is unavailable (HTTP ${response.status}).`);
        const data = await response.json();
        const usb = data?.owner_usb_sync;
        const stateUsb = data?.owner_state_usb_sync;
        if (data?.schema_version !== 2 || !['available', 'missing', 'unavailable'].includes(data.archive_root_state) || !['active', 'inactive', 'unknown'].includes(data.timer_state) || !['active', 'inactive', 'unknown'].includes(data.state_only_timer_state) || !['succeeded', 'failed', 'running', 'never', 'unknown'].includes(data.state_only_export_state) || (data.state_only_export_last_attempt !== null && data.state_only_export_last_attempt !== undefined && (typeof data.state_only_export_last_attempt !== 'string' || data.state_only_export_last_attempt.length > 128)) || !Number.isInteger(data.state_only_archive_count) || data.state_only_archive_count < 0 || !['recent', 'stale', 'unknown'].includes(data.freshness) || !Number.isInteger(data.archive_count) || data.archive_count < 0 || (data.latest_archive !== null && (!data.latest_archive || !Number.isFinite(data.latest_archive.modified_at_unix) || !Number.isFinite(data.latest_archive.size_bytes))) || (data.state_only_latest_archive !== null && (!data.state_only_latest_archive || !Number.isFinite(data.state_only_latest_archive.modified_at_unix) || !Number.isFinite(data.state_only_latest_archive.size_bytes))) || (usb !== null && usb !== undefined && (!['recent', 'stale'].includes(usb.freshness) || !Number.isFinite(usb.last_success_unix) || typeof usb.latest_archive !== 'string' || !/^[0-9a-f]{64}$/.test(usb.latest_archive_sha256) || !Number.isInteger(usb.verified_archive_count) || !Number.isInteger(usb.unverified_archive_count) || !Number.isFinite(usb.retained_archive_payload_bytes)))) throw new Error('The recovery service returned an invalid status.');
        if (stateUsb !== null && stateUsb !== undefined && (!['recent', 'stale'].includes(stateUsb.freshness) || !Number.isFinite(stateUsb.last_success_unix) || typeof stateUsb.latest_archive !== 'string' || !/^[0-9a-f]{64}$/.test(stateUsb.latest_archive_sha256) || !Number.isInteger(stateUsb.verified_generation_count) || !Number.isFinite(stateUsb.retained_payload_bytes) || !/^[0-9a-f]{40}$/.test(stateUsb.source_commit) || stateUsb.latest_rehashed !== true || stateUsb.recovery_kit_available !== true)) throw new Error('The recovery service returned an invalid state-only USB status.');
        const latest = data.latest_archive;
        const stateLatest = data.state_only_latest_archive;
        const stateRecent = stateLatest && (Date.now() / 1000 - stateLatest.modified_at_unix) <= 2 * 60 * 60;
        const freshnessText = data.state_only_timer_state !== 'active' ? 'Automatic state snapshots are not active' : stateRecent ? 'Recent state snapshot' : stateLatest ? 'State snapshot is stale' : 'No state-only snapshot found';
        const freshnessState = data.state_only_timer_state === 'active' && stateRecent && stateLatest.checksum_sidecar_present ? 'success' : 'danger';
        const modified = latest ? new Date(latest.modified_at_unix * 1000).toLocaleString() : 'Not available';
        const stateModified = stateLatest ? new Date(stateLatest.modified_at_unix * 1000).toLocaleString() : 'Not available';
        const usbSync = data.owner_usb_sync;
        const stateUsbSync = data.owner_state_usb_sync;
        const stateUsbState = stateUsbSync?.freshness === 'recent' ? 'success' : 'danger';
        const exportStateText = ({ succeeded: 'Succeeded', failed: 'Failed', running: 'In progress', never: 'Never run', unknown: 'Unknown' })[data.state_only_export_state];
        const exportStateColor = data.state_only_export_state === 'succeeded' ? 'success' : data.state_only_export_state === 'running' ? 'warning' : 'danger';
        const primary = document.createElement('div'); primary.className = 'recovery-facts';
        primary.append(
          fact('State snapshot', freshnessText, freshnessState),
          fact('Latest', stateLatest ? `${humanSize(stateLatest.size_bytes)} · ${stateModified} · checksum ${stateLatest.checksum_sidecar_present ? 'listed' : 'missing'}` : 'Not available', stateLatest?.checksum_sidecar_present ? '' : 'danger'),
          fact('Automatic schedule', data.state_only_timer_state === 'active' ? 'Hourly · active' : data.state_only_timer_state === 'inactive' ? 'Stopped' : 'Unknown', data.state_only_timer_state === 'active' ? 'success' : 'danger'),
          fact('Latest export attempt', `${exportStateText}${data.state_only_export_last_attempt ? ` · ${data.state_only_export_last_attempt}` : ''}`, exportStateColor),
          fact('Retained', String(data.state_only_archive_count)),
          fact('State-only USB copy', stateUsbSync ? (stateUsbSync.freshness === 'recent' ? 'Recently verified' : 'Report is stale') : 'Not reported', stateUsbState)
        );
        const stateUsbDetails = document.createElement('details'); stateUsbDetails.className = 'recovery-details';
        const stateUsbSummary = document.createElement('summary'); stateUsbSummary.textContent = 'State-only USB verification details';
        const stateUsbFacts = document.createElement('dl'); stateUsbFacts.className = 'recovery-facts';
        stateUsbFacts.append(
          fact('Last sync', stateUsbSync ? new Date(stateUsbSync.last_success_unix * 1000).toLocaleString() : 'Not reported'),
          fact('Archive', stateUsbSync ? `${stateUsbSync.latest_archive} · SHA-256 ${stateUsbSync.latest_archive_sha256}` : 'Not reported'),
          fact('Verified data', stateUsbSync ? `${stateUsbSync.verified_generation_count} generations · ${humanSize(stateUsbSync.retained_payload_bytes)}` : 'Not reported'),
          fact('Source-pinned recovery kit', stateUsbSync ? `${stateUsbSync.recovery_kit_available ? 'Available' : 'Missing'} · ${stateUsbSync.source_commit}` : 'Not reported')
        );
        stateUsbDetails.append(stateUsbSummary, stateUsbFacts);
        const legacy = document.createElement('details'); legacy.className = 'recovery-details';
        const legacySummary = document.createElement('summary'); legacySummary.textContent = `Legacy full-host archives · ${data.archive_count}`;
        const legacyFacts = document.createElement('dl'); legacyFacts.className = 'recovery-facts';
        legacyFacts.append(
          fact('Archive folder', data.archive_root_state === 'available' ? 'Readable' : data.archive_root_state === 'missing' ? 'Missing' : 'Unavailable'),
          fact('Latest legacy archive', latest ? `${humanSize(latest.size_bytes)} · ${modified}` : 'Not available'),
          fact('Source revision', latest?.commit_short || 'Not available'),
          fact('Checksum sidecar', latest ? (latest.checksum_sidecar_present ? 'Present; not rechecked here' : 'Missing') : 'Not available'),
          fact('Old schedule', data.timer_state === 'active' ? 'Active' : data.timer_state === 'inactive' ? 'Inactive' : 'Unknown'),
          fact('Next run', typeof data.next_run_display === 'string' && data.next_run_display ? data.next_run_display : 'Not available')
        );
        legacy.append(legacySummary, legacyFacts);
        const legacyUsbDetails = document.createElement('details'); legacyUsbDetails.className = 'recovery-details';
        const legacyUsbSummary = document.createElement('summary'); legacyUsbSummary.textContent = 'Legacy full-host USB verification';
        const legacyUsbFacts = document.createElement('dl'); legacyUsbFacts.className = 'recovery-facts';
        legacyUsbFacts.append(
          fact('Last sync', usbSync ? new Date(usbSync.last_success_unix * 1000).toLocaleString() : 'Not reported'),
          fact('Archive', usbSync ? `${usbSync.latest_archive} · SHA-256 ${usbSync.latest_archive_sha256}` : 'Not reported'),
          fact('Verified data', usbSync ? `${usbSync.verified_archive_count} verified · ${usbSync.unverified_archive_count} unverified · ${humanSize(usbSync.retained_archive_payload_bytes)}` : 'Not reported'),
          fact('Old recovery kit', usbSync?.recovery_kit_available ? `Preflight passed · ${usbSync.recovery_kit_source_commit}` : usbSync ? 'Not included in latest report' : 'Not reported')
        );
        legacyUsbDetails.append(legacyUsbSummary, legacyUsbFacts);
        legacy.append(legacyUsbDetails);
        recoveryFacts.replaceChildren(primary, stateUsbDetails, legacy);
        recoveryFeedback.textContent = 'State-only snapshots contain allowlisted service data. Legacy full-host archives are retained separately during migration.';
      } catch (error) {
        recoveryFacts.replaceChildren(); recoveryFeedback.textContent = error.message || 'Recovery status is unavailable.';
      }
      if (recoveryRefresh) recoveryRefresh.disabled = false;
    };
    recoveryRefresh?.addEventListener('click', refreshRecovery); refreshRecovery();
    recoverySnapshot?.addEventListener('click', async () => {
      if (!window.confirm('Create and validate a fresh state-only snapshot now? This does not copy it to USB or restore any data.')) return;
      recoverySnapshot.disabled = true;
      recoveryFeedback.textContent = 'Queuing a fresh state snapshot…';
      try {
        const before = await fetch('/api/admin/recovery/status', { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(6000) });
        if (!before.ok) throw new Error(`Recovery status is unavailable (HTTP ${before.status}).`);
        const baseline = await before.json();
        const queued = await fetch('/api/admin/recovery/snapshot', {
          method: 'POST', credentials: 'same-origin', cache: 'no-store',
          headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ confirmed: true }),
          signal: AbortSignal.timeout(12000)
        });
        const result = await queued.json().catch(() => ({}));
        if (queued.status === 409) throw new Error('A state snapshot is already running. Refreshing its status…');
        if (!queued.ok || result?.state !== 'queued') throw new Error(queued.status === 401 || queued.status === 403 ? 'A same-origin root sign-in is required.' : `Snapshot request failed (HTTP ${queued.status}).`);
        recoveryFeedback.textContent = 'Snapshot queued. Waiting for validation…';
        const baselineCount = Number(baseline.state_only_archive_count || 0);
        const baselineModified = Number(baseline.state_only_latest_archive?.modified_at_unix || 0);
        for (let attempt = 0; attempt < 45; attempt++) {
          await new Promise(resolve => setTimeout(resolve, 2000));
          const current = await fetch('/api/admin/recovery/status', { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(6000) });
          if (!current.ok) continue;
          const data = await current.json();
          const latestModified = Number(data.state_only_latest_archive?.modified_at_unix || 0);
          if (Number(data.state_only_archive_count) > baselineCount || latestModified > baselineModified) {
            await refreshRecovery();
            recoveryFeedback.textContent = 'Fresh state snapshot published and visible in the verified archive index.';
            return;
          }
        }
        recoveryFeedback.textContent = 'The request was queued, but this page could not confirm a new verified snapshot within 90 seconds. Refresh status and inspect the server journal if it remains unchanged.';
      } catch (error) {
        await refreshRecovery();
        recoveryFeedback.textContent = error.message || 'Could not request a fresh snapshot.';
      } finally {
        recoverySnapshot.disabled = false;
      }
    });
  }

  const fleetList = document.querySelector('#server-fleet-list');
  const fleetSummary = document.querySelector('#server-fleet-summary');
  const fleetRefresh = document.querySelector('#server-fleet-refresh');
  const serverActionTarget = document.querySelector('#server-action-target');
  const serverModuleActions = document.querySelector('#server-module-actions');
  const serverRebootButton = document.querySelector('#server-action-reboot');
  const serverActionStatus = document.querySelector('#server-action-status');
  const serverOperationList = document.querySelector('#server-operation-list');
  const serverLabelById = new Map();
    const nodeForm = document.querySelector('#server-node-connect-form');
    const nodeSlot = document.querySelector('#server-node-slot');
    const nodeLabel = document.querySelector('#server-node-label');
    const nodeRole = document.querySelector('#server-node-role');
    const nodeStatus = document.querySelector('#server-node-management-status');
    const nodeManagement = document.querySelector('#server-node-management');
    const nodeSecretPanel = document.querySelector('#server-node-secret');
    const nodeSecretValue = document.querySelector('#server-node-secret-value');
    const nodeSecretDownload = document.querySelector('#server-node-secret-download');
    const nodeSecretClose = document.querySelector('#server-node-secret-close');
    const nodeStartNew = document.querySelector('#server-node-start-new');
    const nodeStartReplacement = document.querySelector('#server-node-start-replacement');
    const nodeCancel = document.querySelector('#server-node-cancel');
    const nodeFormTitle = document.querySelector('#server-node-form-title');
    const nodeFormCopy = document.querySelector('#server-node-form-copy');
    const nodeSubmit = document.querySelector('#server-node-submit');
    const nodeSecretReveal = document.querySelector('#server-node-secret-reveal');
    const selectedServerName = document.querySelector('#server-selected-name');
    const selectedServerState = document.querySelector('#server-selected-state');
    const selectedServerOverview = document.querySelector('#server-selected-overview');
  const networkPanel = document.querySelector('#server-network-panel');
  const networkStatus = document.querySelector('#server-network-status');
  const networkDetails = document.querySelector('#server-network-details');
  const networkRange = document.querySelector('#server-network-range');
  const networkCanvas = document.querySelector('#server-network-chart');
  const networkRefresh = document.querySelector('#server-network-refresh');
    let createdNodeConfig = null;
    let connectionMode = 'new';
    let selectedServerId = null;
    let lastFleetData = null;
    let serverOperationData = null;
  if (fleetList && fleetSummary) {
    const serverTabs = [...document.querySelectorAll('[data-server-tab]')];
    const serverPanels = [...document.querySelectorAll('[data-server-panel]')];
    const activateServerTool = name => {
      for (const tab of serverTabs) {
        const active = tab.dataset.serverTab === name;
        tab.setAttribute('aria-selected', String(active)); tab.tabIndex = active ? 0 : -1;
      }
      for (const panel of serverPanels) panel.hidden = panel.dataset.serverPanel !== name;
      if (name === 'diagnostics' && selectedServerId) refreshNetwork();
    };
    for (const tab of serverTabs) {
      tab.addEventListener('click', () => activateServerTool(tab.dataset.serverTab));
      tab.addEventListener('keydown', event => {
        const index = serverTabs.indexOf(tab);
        const next = event.key === 'ArrowRight' ? (index + 1) % serverTabs.length : event.key === 'ArrowLeft' ? (index + serverTabs.length - 1) % serverTabs.length : event.key === 'Home' ? 0 : event.key === 'End' ? serverTabs.length - 1 : -1;
        if (next >= 0) { event.preventDefault(); serverTabs[next].focus(); activateServerTool(serverTabs[next].dataset.serverTab); }
      });
    }
    const renderSelectedServer = node => {
      if (!node) return;
      if (networkRequest && networkRequestServerId !== node.id) { networkRefreshKey = `${node.id}:${networkRange?.value || '1h'}`; networkRequest.abort(); }
      selectedServerId = node.id;
      if (selectedServerName) selectedServerName.textContent = node.label || 'Unnamed server';
      const stateLabels = { online: 'Online', stale: 'Stale telemetry', revoked: 'Disconnected', 'never-reported': 'Waiting for first check-in', unavailable: 'Unavailable' };
      const controlState = node.id === lastFleetData?.local_node_id || node.executor_online ? 'typed controls ready' : 'telemetry only · typed controls unavailable';
      if (selectedServerState) selectedServerState.textContent = `${stateLabels[node.state] || 'Unavailable'} · ${node.role || 'server'} · ${controlState}`;
      if (selectedServerOverview) {
        selectedServerOverview.replaceChildren();
        const metrics = document.createElement('dl'); metrics.className = 'server-node-metrics';
        metric(metrics, 'CPU · host total', Number.isFinite(node.cpu_percent) ? `${Number(node.cpu_percent).toFixed(1)}% · ${node.logical_cpu_count || '?'} cores` : 'Unavailable');
        metric(metrics, 'Memory · host total', node.memory_total_bytes != null && node.memory_available_bytes != null ? `${formatBytes(Number(node.memory_total_bytes) - Number(node.memory_available_bytes))} used / ${formatBytes(node.memory_total_bytes)}` : 'Unavailable');
        metric(metrics, 'Uptime', formatUptime(node.uptime_seconds));
        metric(metrics, 'Load · 1 min', Number.isFinite(node.load_average_1m) ? Number(node.load_average_1m).toFixed(2) : 'Unavailable');
        selectedServerOverview.append(metrics);
      }
      for (const button of fleetList.querySelectorAll('[data-node-id]')) button.setAttribute('aria-pressed', String(button.dataset.nodeId === selectedServerId));
      if (serverActionTarget) {
        const option = [...serverActionTarget.options].find(candidate => candidate.value === node.id);
        if (option && !option.disabled) serverActionTarget.value = node.id;
        else serverActionTarget.value = '';
        syncTargetControls();
      }
      if (serverOperationData) renderServerOperations(serverOperationData);
      if (networkPanel && !networkPanel.hidden) refreshNetwork();
    };
    const formatBytes = value => {
      let amount = Number(value);
      if (!Number.isFinite(amount) || amount < 0) return 'Unavailable';
      const units = ['B', 'KB', 'MB', 'GB', 'TB']; let index = 0;
      while (amount >= 1024 && index < units.length - 1) { amount /= 1024; index++; }
      return `${amount.toFixed(index === 0 ? 0 : 1)} ${units[index]}`;
    };
    const formatUptime = value => {
      const seconds = Number(value);
      if (!Number.isFinite(seconds) || seconds < 0) return 'Unavailable';
      const days = Math.floor(seconds / 86400); const hours = Math.floor((seconds % 86400) / 3600);
      const minutes = Math.floor((seconds % 3600) / 60);
      return days ? `${days}d ${hours}h` : hours ? `${hours}h ${minutes}m` : `${minutes}m`;
    };
    const metric = (parent, label, value) => {
      const cell = document.createElement('div'); const term = document.createElement('dt'); const detail = document.createElement('dd');
      term.textContent = label; detail.textContent = value; cell.append(term, detail); parent.append(cell);
    };
    const renderFleet = data => {
      if (data?.schema_version !== 1 || !['local-platform-host-only', 'platform-host-and-enrolled-nodes'].includes(data?.scope) || typeof data.local_node_id !== 'string' || !Array.isArray(data.nodes) || !Number.isInteger(data.reporting_node_count)) throw new Error('The server inventory response has an unsupported format.');
      lastFleetData = data;
      fleetList.replaceChildren();
      const reporting = data.nodes.filter(node => node.state === 'online').length;
      fleetSummary.textContent = `${reporting} of ${data.nodes.length} registered ${data.nodes.length === 1 ? 'server is' : 'servers are'} reporting · sampled ${new Date(Number(data.observed_at_unix) * 1000).toLocaleTimeString()}`;
      serverLabelById.clear();
      serverLabelById.set(data.local_node_id, 'Core server');
      for (const node of data.nodes) {
        serverLabelById.set(node.id, node.label || node.id);
        const card = document.createElement('article'); card.className = 'server-node card';
        const select = document.createElement('button'); select.type = 'button'; select.className = 'server-node-select'; select.dataset.nodeId = node.id; select.setAttribute('aria-pressed', String(node.id === selectedServerId));
        const header = document.createElement('div'); header.className = 'server-node-header';
        const title = document.createElement('h3'); title.textContent = node.label || node.id || 'Unnamed server';
        const stateLabels = { online: 'Online', stale: 'Stale', revoked: 'Revoked', 'never-reported': 'Never reported', unavailable: 'Unavailable' };
        const status = document.createElement('span'); status.className = `server-node-status${node.state === 'online' ? '' : node.state === 'stale' ? ' stale' : ' unavailable'}`;
        status.textContent = stateLabels[node.state] || 'Unavailable';
        header.append(title, status); select.append(header);
        const role = document.createElement('p'); role.className = 'server-node-role'; role.textContent = node.role || 'server';
        const metrics = document.createElement('dl'); metrics.className = 'server-node-metrics';
        metric(metrics, 'CPU', Number.isFinite(node.cpu_percent) ? `${Number(node.cpu_percent).toFixed(1)}% · ${node.logical_cpu_count || '?'} cores` : `Sampling · ${node.logical_cpu_count || '?'} cores`);
        metric(metrics, 'Memory', node.memory_total_bytes != null && node.memory_available_bytes != null ? `${formatBytes(Number(node.memory_total_bytes) - Number(node.memory_available_bytes))} used / ${formatBytes(node.memory_total_bytes)}` : 'Unavailable');
        metric(metrics, 'Uptime', formatUptime(node.uptime_seconds));
        metric(metrics, 'Load · 1 min', Number.isFinite(node.load_average_1m) ? Number(node.load_average_1m).toFixed(2) : 'Unavailable');
        const freshness = document.createElement('p'); freshness.className = 'server-fleet-note';
        const age = Math.max(0, Math.floor(Date.now() / 1000) - Number(node.observed_at_unix || data.observed_at_unix));
        freshness.textContent = node.state === 'never-reported' ? 'No telemetry received yet.' : `Received ${age < 5 ? 'just now' : `${age}s ago`} · metrics ${node.metrics_state || 'unknown'}`;
        select.addEventListener('click', () => renderSelectedServer(node)); card.append(select, role, metrics, freshness); fleetList.append(card);
      }
      if (serverActionTarget && data.local_node_id) {
        serverActionTarget.dataset.localNodeId = data.local_node_id;
        if (!serverActionTarget.options.length) serverActionTarget.add(new Option('Core server · this Platform host', data.local_node_id));
      }
      if (data.nodes.length === 0) fleetList.append(appendText(document.createElement('div'), 'p', 'No servers have been added yet.', 'muted'));
      const selected = data.nodes.find(node => node.id === selectedServerId) || data.nodes.find(node => node.id === data.local_node_id) || data.nodes[0];
      if (selected) renderSelectedServer(selected);
    };
    const refreshFleet = async () => {
      if (fleetRefresh) fleetRefresh.disabled = true;
      fleetSummary.textContent = 'Refreshing server inventory…';
      try {
        const response = await fetch('/api/admin/servers', { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(6000) });
        const data = await response.json().catch(() => null);
        if (!response.ok) throw new Error(response.status === 401 || response.status === 403 ? 'Root sign-in is required to view infrastructure.' : `Server inventory is unavailable (HTTP ${response.status}).`);
        renderFleet(data);
      } catch (error) {
        fleetList.replaceChildren(); fleetSummary.textContent = error.message || 'Server inventory is unavailable.';
      } finally { if (fleetRefresh) fleetRefresh.disabled = false; }
    };
    fleetRefresh?.addEventListener('click', refreshFleet); refreshFleet();

    let networkRequest = null;
    let networkRequestServerId = null;
    let networkRequestKey = null;
    let networkRefreshKey = null;
    let lastNetworkData = null;
    const drawNetworkChart = samples => {
      if (!networkCanvas) return;
      const context = networkCanvas.getContext('2d'); if (!context) return;
      const width = networkCanvas.clientWidth || 640; const height = 120; const ratio = window.devicePixelRatio || 1;
      networkCanvas.width = Math.round(width * ratio); networkCanvas.height = Math.round(height * ratio);
      context.scale(ratio, ratio); context.clearRect(0, 0, width, height);
      context.strokeStyle = '#526070'; context.lineWidth = 1; context.beginPath(); context.moveTo(0, height - 18); context.lineTo(width, height - 18); context.stroke();
      if (!Array.isArray(samples) || samples.length < 2) return;
      const rates = key => samples.slice(1).map((sample, index) => {
        const previous = samples[index]; const elapsed = Number(sample.sampled_at_unix) - Number(previous.sampled_at_unix);
        const delta = Number(sample[key]) - Number(previous[key]);
        return elapsed > 0 && delta >= 0 ? delta / elapsed : null;
      }).filter(Number.isFinite);
      const inbound = rates('rx_bytes'); const outbound = rates('tx_bytes');
      if (inbound.length < 2 || outbound.length < 2) return;
      const maximum = Math.max(...inbound, ...outbound, 1);
      for (const [series, color] of [[inbound, '#77d6a7'], [outbound, '#79b8ff']]) {
        context.strokeStyle = color; context.lineWidth = 2; context.beginPath();
        series.forEach((rate, index) => { const x = index * (width / Math.max(1, series.length - 1)); const y = height - 20 - ((rate / maximum) * (height - 34)); index ? context.lineTo(x, y) : context.moveTo(x, y); }); context.stroke();
      }
    };
    const refreshNetwork = async () => {
      if (!networkPanel || !selectedServerId) return;
      const serverId = selectedServerId; const range = networkRange?.value || '1h'; const requestKey = `${serverId}:${range}`;
      if (networkRequest) {
        if (networkRequestKey !== requestKey) { networkRefreshKey = requestKey; networkRequest.abort(); }
        return;
      }
      networkRequest = new AbortController(); networkRequestServerId = serverId; networkRequestKey = requestKey; if (networkRefresh) networkRefresh.disabled = true;
      if (networkStatus) networkStatus.textContent = 'Loading network sample…';
      try {
        const response = await fetch(`/api/admin/server-nodes/${encodeURIComponent(serverId)}/network?window=${encodeURIComponent(range)}`, { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.any([networkRequest.signal, AbortSignal.timeout(6000)]) });
        const data = await response.json().catch(() => null);
        if (serverId !== selectedServerId || range !== (networkRange?.value || '1h')) return;
        if (!response.ok) throw new Error(response.status === 404 ? 'No network sample is available for this server yet.' : `Network diagnostics unavailable (HTTP ${response.status}).`);
        lastNetworkData = data;
        const sample = data.network;
        networkDetails.replaceChildren();
        if (sample) {
          const interfaces = document.createElement('div'); interfaces.className = 'server-network-interfaces';
          for (const item of sample.interfaces || []) {
            const card = document.createElement('article'); card.className = 'server-network-interface';
            const title = document.createElement('strong'); title.textContent = item.name;
            const direction = document.createElement('p'); direction.textContent = `${item.loopback ? 'Loopback' : 'Network'} · RX ${formatBytes(Number(item.rx_bytes))} · TX ${formatBytes(Number(item.tx_bytes))}`;
            const health = document.createElement('p'); health.textContent = `Errors ${Number(item.rx_errors) + Number(item.tx_errors)} · drops ${Number(item.rx_dropped) + Number(item.tx_dropped)} · packets ${Number(item.rx_packets) + Number(item.tx_packets)}`;
            card.append(title, direction, health); interfaces.append(card);
          }
          const states = document.createElement('p'); states.className = 'muted';
          const counts = sample.tcp_states || {}; const otherStates = Object.entries(counts).filter(([key]) => !['established', 'listen'].includes(key)).reduce((total, [, count]) => total + Number(count || 0), 0);
          states.textContent = `TCP sockets · established ${Number(counts.established || 0)} · listening ${Number(counts.listen || 0)} · other states ${otherStates}`;
          networkDetails.append(interfaces, states);
          const age = data.observed_at_unix == null ? null : Math.max(0, Math.floor(Date.now() / 1000) - Number(data.observed_at_unix));
          const points = data.samples || []; const previous = points.length > 1 ? points[points.length - 2] : null; const latest = points.at(-1);
          const elapsed = previous && latest ? Number(latest.sampled_at_unix) - Number(previous.sampled_at_unix) : 0;
          const rate = (key) => { const delta = previous && latest ? Number(latest[key]) - Number(previous[key]) : -1; return elapsed > 0 && delta >= 0 ? `${formatBytes(delta / elapsed)}/s` : '—'; };
          networkStatus.textContent = age == null ? 'Interface counters have not been reported.' : `${age > 90 ? 'STALE · ' : ''}Latest sample ${age < 10 ? 'just now' : `${age}s ago`} · RX ${rate('rx_bytes')} · TX ${rate('tx_bytes')} · chart green RX / blue TX`;
        } else {
          networkDetails.append(appendText(document.createElement('div'), 'p', 'Network metrics are unavailable; this agent may need an update.', 'muted'));
          networkStatus.textContent = 'No compatible network sample.';
        }
        drawNetworkChart(data.samples || []);
      } catch (error) { if (error.name !== 'AbortError' && networkStatus) networkStatus.textContent = error.message || 'Network diagnostics unavailable.'; }
      finally {
        networkRequest = null; networkRequestServerId = null; networkRequestKey = null; if (networkRefresh) networkRefresh.disabled = false;
        if (networkRefreshKey && networkPanel && !networkPanel.hidden) { networkRefreshKey = null; refreshNetwork(); }
      }
    };
    networkRefresh?.addEventListener('click', refreshNetwork);
    networkRange?.addEventListener('change', refreshNetwork);
    window.addEventListener('resize', () => { if (networkPanel && !networkPanel.hidden && lastNetworkData) drawNetworkChart(lastNetworkData.samples || []); }, { passive: true });

    const renderServerOperations = data => {
      if (!serverOperationList) return;
      if (!Array.isArray(data?.items)) throw new Error('The operation history response has an unsupported format.');
      serverOperationData = data;
      serverOperationList.replaceChildren();
      const visibleItems = (selectedServerId ? data.items.filter(operation => operation.target_node_id === selectedServerId) : data.items).slice(0, 12);
      for (const operation of visibleItems) {
        const item = document.createElement('article'); item.className = 'server-operation-item';
        const action = operation.action?.kind === 'restart_service' ? `Restart ${operation.action.service}` : operation.action?.kind === 'reboot_host' ? 'Reboot host' : operation.action?.kind || 'Unknown action';
        const title = document.createElement('strong'); title.textContent = action;
        const state = document.createElement('span'); state.className = `server-operation-state ${operation.state || ''}`; state.textContent = operation.state || 'unknown';
        const detail = document.createElement('small'); detail.textContent = `${serverLabelById.get(operation.target_node_id) || operation.target_node_id} · ${operation.result_detail || `Requested ${new Date(Number(operation.created_at_unix) * 1000).toLocaleString()}`}`;
        item.append(title, state, detail); serverOperationList.append(item);
      }
      if (!visibleItems.length) serverOperationList.append(appendText(document.createElement('div'), 'p', 'No actions recorded for this server.', 'muted'));
    };
    const refreshServerOperations = async () => {
      if (!serverOperationList) return;
      try {
        const response = await fetch('/api/admin/server-operations', { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(6000) });
        const data = await response.json().catch(() => null);
        if (!response.ok) throw new Error(response.status === 401 || response.status === 403 ? 'Root sign-in is required to operate servers.' : `Action history is unavailable (HTTP ${response.status}).`);
        renderServerOperations(data);
      } catch (error) { serverOperationList.replaceChildren(appendText(document.createElement('div'), 'p', error.message || 'Action history is unavailable.', 'muted')); }
    };
    refreshServerOperations();
    window.setInterval(() => {
      if (!document.hidden && serverOperationList?.querySelector('.server-operation-state.accepted, .server-operation-state.running')) refreshServerOperations();
    }, 5000);
    const syncTargetControls = () => {
      if (!serverActionTarget) return;
      const hasTarget = Boolean(serverActionTarget.value);
      const local = hasTarget && serverActionTarget.value === serverActionTarget.dataset.localNodeId;
      for (const button of serverModuleActions?.querySelectorAll('[data-service]') || []) button.disabled = !hasTarget || (!local && button.dataset.service === 'caddy');
      if (serverRebootButton) serverRebootButton.disabled = !local;
    };
    serverActionTarget?.addEventListener('change', () => {
      const node = lastFleetData?.nodes?.find(candidate => candidate.id === serverActionTarget.value);
      if (node) renderSelectedServer(node);
      else syncTargetControls();
    });
    const submitServerAction = async (action, description) => {
      const target = serverActionTarget?.value || '';
      if (!target) { serverActionStatus.textContent = 'Choose an available host first.'; return; }
      if (serverRebootButton) serverRebootButton.disabled = true;
      serverActionStatus.textContent = `Requesting ${description}…`;
      try {
        const response = await fetch('/api/admin/server-operations', { method: 'POST', credentials: 'same-origin', cache: 'no-store', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ client_request_id: crypto.randomUUID(), target_node_id: target, action, confirmed: true }), signal: AbortSignal.timeout(15000) });
        const data = await response.json().catch(() => null);
        if (!response.ok) throw new Error(data?.error || `Action request failed (HTTP ${response.status}).`);
        const operation = data.operation;
        serverActionStatus.textContent = operation.state === 'accepted' ? `${description} queued for the selected server.` : `${operation.state}: ${operation.result_detail || description}`;
        if (operation.state === 'awaiting_reboot') serverActionStatus.textContent = 'Reboot requested. This page will disconnect; the operation will be reconciled after the host returns.';
        await refreshServerOperations();
      } catch (error) {
        serverActionStatus.textContent = `${error.message || 'The action result is unknown.'} Check action history before retrying.`;
        await refreshServerOperations();
      } finally {
        if (serverRebootButton) serverRebootButton.disabled = false;
      }
    };
    serverModuleActions?.addEventListener('click', async event => {
      const button = event.target.closest('button[data-service]');
      if (!button || button.disabled) return;
      const service = button.dataset.service;
      const label = button.dataset.label || service;
      const target = serverActionTarget?.selectedOptions?.[0]?.textContent || 'selected server';
      if (!serverActionTarget?.value) { serverActionStatus.textContent = 'Choose a machine with an available typed executor first.'; return; }
      if (!window.confirm(`Restart ${label} on ${target}? This records and queues a typed action.`)) return;
      button.disabled = true;
      try { await submitServerAction({ kind: 'restart_service', service }, `restart of ${label}`); }
      finally { syncTargetControls(); }
    });
    serverRebootButton?.addEventListener('click', async () => {
      const target = serverActionTarget?.value || '';
      if (!target || target !== serverActionTarget?.dataset.localNodeId) { serverActionStatus.textContent = 'Host reboot is currently available only for the core server.'; return; }
      const expected = `REBOOT ${target}`;
      const confirmation = window.prompt(`This will reboot the Platform host. Type exactly:\n${expected}`);
      if (confirmation !== expected) { serverActionStatus.textContent = 'Reboot cancelled; confirmation did not match.'; return; }
      await submitServerAction({ kind: 'reboot_host', confirmation }, 'host reboot');
    });

    const postNodeApi = async (path, body) => {
      const response = await fetch(path, { method: 'POST', credentials: 'same-origin', cache: 'no-store', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body), signal: AbortSignal.timeout(8000) });
      const result = await response.json().catch(() => null);
      if (!response.ok) throw new Error(result?.error || `Request failed (HTTP ${response.status}).`);
      return result;
    };
    const refreshNodeManagement = async () => {
      if (!nodeStatus || !nodeManagement) return;
      nodeStatus.textContent = 'Refreshing server records…';
      try {
        const response = await fetch('/api/admin/server-nodes', { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(6000) });
        const data = await response.json().catch(() => null);
        if (!response.ok || data?.schema_version !== 1 || !Array.isArray(data.slots) || !Array.isArray(data.nodes) || !Array.isArray(data.events)) throw new Error(response.status === 401 || response.status === 403 ? 'Root sign-in is required.' : `Server records are unavailable (HTTP ${response.status}).`);
        nodeManagement.replaceChildren();
        const slotHeading = document.createElement('h3'); slotHeading.textContent = 'Logical server slots'; nodeManagement.append(slotHeading);
        nodeSlot.replaceChildren(new Option('Create a new logical slot', ''));
        for (const slot of data.slots) {
          if (slot.status === 'vacant') {
            const option = new Option(`${slot.label} · ${slot.role} · ${slot.id}`, slot.id);
            option.dataset.label = slot.label; option.dataset.role = slot.role; nodeSlot.add(option);
          }
          const row = document.createElement('article'); row.className = 'server-node-management-row';
          const details = document.createElement('div');
          const title = document.createElement('strong'); title.textContent = slot.label;
          const meta = document.createElement('p'); meta.className = 'muted'; meta.textContent = `${slot.role} · ${slot.status} · ${slot.current_node_id ? `current installation ${slot.current_node_id}` : 'no active installation'} · slot ${slot.id}`;
          details.append(title, meta); row.append(details); nodeManagement.append(row);
        }
        if (nodeStartReplacement) nodeStartReplacement.disabled = nodeSlot.options.length <= 1;
        const incarnationHeading = document.createElement('h3'); incarnationHeading.textContent = 'Host installations'; nodeManagement.append(incarnationHeading);
        for (const node of data.nodes) {
          const fleetNode = lastFleetData?.nodes?.find(candidate => candidate.id === node.id);
          if (fleetNode) fleetNode.executor_online = node.status === 'active' && node.executor_enabled && node.executor_online;
        }
        if (serverActionTarget) {
          const selectedTarget = serverActionTarget.value;
          serverActionTarget.replaceChildren();
          const localId = serverActionTarget.dataset.localNodeId;
          if (localId) serverActionTarget.add(new Option('Core server · this Platform host', localId));
          for (const node of data.nodes) {
            if (node.status === 'active' && node.executor_enabled && node.executor_online) {
              serverActionTarget.add(new Option(`${node.label} · ${node.role}`, node.id));
            }
          }
          const desiredTarget = selectedServerId || selectedTarget;
          if ([...serverActionTarget.options].some(option => option.value === desiredTarget)) serverActionTarget.value = desiredTarget;
          else serverActionTarget.value = '';
          syncTargetControls();
        }
        for (const node of data.nodes) {
          const row = document.createElement('article'); row.className = 'server-node-management-row';
          const details = document.createElement('div');
          const title = document.createElement('strong'); title.textContent = node.label;
          const capability = node.status !== 'active' ? 'revoked' : node.executor_online ? 'telemetry + typed actions · online' : node.executor_enabled ? 'executor configured · offline' : 'telemetry only';
          const meta = document.createElement('p'); meta.className = 'muted'; meta.textContent = `${node.role} · ${capability} · ${node.status} · slot ${node.slot_id} · installation ${node.id} · ${node.last_report_received_at_unix ? `last report ${new Date(node.last_report_received_at_unix * 1000).toLocaleString()}` : 'never reported'}`;
          details.append(title, meta); row.append(details);
          if (node.status === 'active') {
            const revoke = document.createElement('button'); revoke.type = 'button'; revoke.textContent = 'Revoke';
            revoke.addEventListener('click', async () => {
              if (!window.confirm(`Revoke ${node.label}? Telemetry and future typed-action claims will stop; an action already running may still finish.`)) return;
              revoke.disabled = true;
              try { await postNodeApi(`/api/admin/server-nodes/${encodeURIComponent(node.id)}/revoke`, { expected_version: node.version }); nodeStatus.textContent = `Telemetry and typed-operation access for ${node.label} revoked.`; await refreshNodeManagement(); await refreshFleet(); }
              catch (error) { nodeStatus.textContent = error.message || 'Could not revoke this node.'; revoke.disabled = false; }
            });
            row.append(revoke);
          }
          nodeManagement.append(row);
        }
        const selectedFleetNode = lastFleetData?.nodes?.find(node => node.id === selectedServerId);
        if (selectedFleetNode) renderSelectedServer(selectedFleetNode);
        const recentEvents = data.events.slice(0, 8);
        if (recentEvents.length) {
          const disclosure = document.createElement('details'); disclosure.className = 'server-node-events';
          const summary = document.createElement('summary'); summary.textContent = `Recent server changes · ${recentEvents.length}`;
          const list = document.createElement('ul');
          for (const event of recentEvents) {
            const item = document.createElement('li'); item.textContent = `${event.event} · ${event.label} (${event.role}) · ${new Date(event.at_unix * 1000).toLocaleString()} · operator ${event.actor_profile_id}`; list.append(item);
          }
          disclosure.append(summary, list); nodeManagement.append(disclosure);
        }
        nodeStatus.textContent = `${data.slots.length} server places · ${data.nodes.length} connections, including history.`;
      } catch (error) { nodeManagement.replaceChildren(); nodeStatus.textContent = error.message || 'Server records are unavailable.'; }
    };
    const syncSlotSelection = () => {
      const selected = nodeSlot?.selectedOptions?.[0];
      const slot = connectionMode === 'replacement' && selected?.value ? selected : null;
      if (slot) {
        nodeLabel.value = slot.dataset.label;
        nodeRole.value = slot.dataset.role;
        nodeLabel.readOnly = true; nodeRole.disabled = true;
      } else {
        nodeLabel.readOnly = false; nodeRole.disabled = false;
      }
    };
    const startConnection = mode => {
      connectionMode = mode;
      if (mode === 'replacement' && nodeSlot.options.length <= 1) {
        nodeStatus.textContent = 'There is no vacant server place to reconnect. Revoke the old installation first, or use Add server.';
        return;
      }
      nodeForm.hidden = false;
      clearCreatedNodeSecret();
      if (mode === 'new') {
        nodeForm.reset(); nodeSlot.value = ''; nodeSlot.disabled = true;
        nodeFormTitle.textContent = 'Add server';
        nodeFormCopy.textContent = 'This machine must already exist with your provider. Vapor prepares its connection only.';
        nodeSubmit.textContent = 'Prepare connection';
      } else {
        nodeSlot.disabled = false;
        nodeForm.reset();
        nodeSlot.selectedIndex = 1;
        nodeFormTitle.textContent = 'Reconnect replacement';
        nodeFormCopy.textContent = 'Use this after wiping or replacing a machine. Its server place and history stay the same.';
        nodeSubmit.textContent = 'Prepare replacement connection';
      }
      syncSlotSelection();
      nodeForm.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
      (mode === 'replacement' ? nodeSlot : nodeLabel).focus();
    };
    nodeStartNew?.addEventListener('click', () => startConnection('new'));
    nodeStartReplacement?.addEventListener('click', () => startConnection('replacement'));
    nodeCancel?.addEventListener('click', () => { nodeForm.reset(); nodeForm.hidden = true; nodeSlot.disabled = false; syncSlotSelection(); });
    nodeSlot?.addEventListener('change', syncSlotSelection);
    nodeForm?.addEventListener('submit', async event => {
      event.preventDefault();
      if (connectionMode === 'replacement' && !nodeSlot.value) { nodeStatus.textContent = 'Choose a vacant server place for the replacement.'; return; }
      const submit = nodeForm.querySelector('button[type="submit"]'); submit.disabled = true; nodeStatus.textContent = 'Preparing the connection…';
      try {
        const request = { label: nodeLabel.value, role: nodeRole.value };
        if (nodeSlot.value) request.slot_id = nodeSlot.value;
        const created = await postNodeApi('/api/admin/server-nodes', request);
        nodeSecretValue.textContent = `Telemetry credential:\n${created.reporter_credential}\n\nTyped executor credential:\n${created.executor_credential}\n\n${created.warning}`;
        createdNodeConfig = { id: created.id, reporterCredential: created.reporter_credential, executorCredential: created.executor_credential };
        nodeSecretValue.hidden = true; nodeSecretReveal.textContent = 'Show one-time credentials'; nodeSecretReveal.setAttribute('aria-expanded', 'false');
        nodeSecretPanel.hidden = false;
        nodeForm.hidden = true;
        nodeStatus.textContent = `Connection prepared for ${created.label}. Download the config, install the node agent, and wait for its first check-in.`;
        nodeForm.reset(); nodeSlot.disabled = false; syncSlotSelection(); await refreshNodeManagement(); await refreshFleet();
      } catch (error) { nodeStatus.textContent = error.message || 'Could not add server.'; }
      finally { submit.disabled = false; }
    });
    nodeSecretReveal?.addEventListener('click', () => {
      const reveal = nodeSecretValue.hidden;
      nodeSecretValue.hidden = !reveal;
      nodeSecretReveal.textContent = reveal ? 'Hide one-time credentials' : 'Show one-time credentials';
      nodeSecretReveal.setAttribute('aria-expanded', String(reveal));
    });
    nodeSecretDownload?.addEventListener('click', () => {
      const current = createdNodeConfig;
      const origin = new URL(window.location.origin);
      const validCredential = credential => { const parts = String(credential || '').split('.'); return parts.length === 4 && parts[0] === 'v1' && parts[1] === current?.id && /^[0-9a-f]{32}$/i.test(parts[2]) && /^[0-9a-f]{32}$/i.test(parts[3]); };
      const validId = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(current?.id || '');
      if (!current || origin.protocol !== 'https:' || !validId || !validCredential(current.reporterCredential) || !validCredential(current.executorCredential) || current.reporterCredential === current.executorCredential) {
        nodeStatus.textContent = 'The one-time server setup data is unavailable or invalid. Add the server again.';
        return;
      }
      const quote = value => JSON.stringify(value);
      const config = `platform_origin = ${quote(origin.origin)}\nnode_id = ${quote(current.id)}\nreporter_credential = ${quote(current.reporterCredential)}\nexecutor_credential = ${quote(current.executorCredential)}\n`;
      const url = URL.createObjectURL(new Blob([config], { type: 'application/toml' }));
      const link = document.createElement('a'); link.href = url; link.download = `vapor-node-${current.id}.toml`; link.click();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
      nodeStatus.textContent = 'Config downloaded. It contains separate telemetry and typed-executor secrets; transfer it privately, install it as root-owned and readable only by the agent group, then delete the extra copy.';
    });
    const clearCreatedNodeSecret = () => { nodeSecretValue.textContent = ''; nodeSecretValue.hidden = true; createdNodeConfig = null; nodeSecretPanel.hidden = true; };
    nodeSecretClose?.addEventListener('click', clearCreatedNodeSecret);
    window.addEventListener('pagehide', clearCreatedNodeSecret, { once: true });
    refreshNodeManagement();
  }

  const consoleStatus = document.querySelector('#emergency-console-status');
  const consoleEvents = document.querySelector('#emergency-console-events');
  const consoleRefresh = document.querySelector('#emergency-console-refresh');
  if (consoleStatus && consoleEvents) {
    const enableButton = document.querySelector('#emergency-console-enable');
    const disableButton = document.querySelector('#emergency-console-disable');
    const startButton = document.querySelector('#emergency-console-start');
    const endButton = document.querySelector('#emergency-console-end');
    const acknowledgement = document.querySelector('#emergency-console-ack');
    const post = async (path, body) => {
      const response = await fetch(path, { method: 'POST', credentials: 'same-origin', cache: 'no-store', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body), signal: AbortSignal.timeout(8000) });
      const result = await response.json().catch(() => null);
      if (!response.ok) throw new Error(result?.error || `Request failed (HTTP ${response.status}).`);
      return result;
    };
    const refreshConsole = async () => {
      consoleRefresh.disabled = true; consoleStatus.textContent = 'Checking console state…';
      try {
        const response = await fetch('/api/admin/emergency-console/status', { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(6000) });
        const data = await response.json().catch(() => null);
        if (!response.ok || typeof data?.configured !== 'boolean' || typeof data?.enabled !== 'boolean' || typeof data?.active !== 'boolean' || !Array.isArray(data.events)) throw new Error(response.status === 401 || response.status === 403 ? 'Root sign-in is required.' : 'Emergency console status is unavailable.');
        if (!data.configured) consoleStatus.textContent = 'Disabled by host configuration. It is not exposed until the root-owned server setting is enabled.';
        else if (!data.enabled) consoleStatus.textContent = 'Available but disabled. Enable it here before starting a session.';
        else if (data.active) consoleStatus.textContent = `Session active until ${data.expires_at_unix ? new Date(data.expires_at_unix * 1000).toLocaleTimeString() : 'expiry unknown'}${data.connected ? ' · terminal connected' : ' · waiting for terminal connection'}.`;
        else consoleStatus.textContent = 'Enabled; no active session.';
        enableButton.hidden = !data.configured || data.enabled;
        disableButton.hidden = !data.enabled;
        startButton.disabled = !data.enabled || data.active || !acknowledgement.checked;
        endButton.hidden = !data.active;
        const eventLabels = { enabled: 'Console access enabled', disabled: 'Console access disabled', started: 'Root session started', start_failed: 'Terminal failed to start', connected: 'Terminal connected', ended: 'Root session ended', revoked: 'Session revoked', expired: 'Session expired', stop_failed: 'Terminal stop failed' };
        consoleEvents.replaceChildren(...data.events.map(item => { const row = document.createElement('li'); row.textContent = `${eventLabels[item.event] || 'Console event'} · ${new Date(item.at_unix * 1000).toLocaleString()}`; return row; }));
      } catch (error) { consoleStatus.textContent = error.message || 'Emergency console status is unavailable.'; consoleEvents.replaceChildren(); startButton.disabled = true; }
      consoleRefresh.disabled = false;
    };
    acknowledgement?.addEventListener('change', refreshConsole);
    enableButton?.addEventListener('click', async () => { if (!window.confirm('Enable root browser-console sessions?')) return; consoleStatus.textContent = 'Enabling console sessions…'; try { await post('/api/admin/emergency-console/enable', { confirmed: true }); } catch (error) { consoleStatus.textContent = error.message || 'Could not enable console sessions.'; } await refreshConsole(); });
    disableButton?.addEventListener('click', async () => { if (!window.confirm('Disable console access and stop any active root terminal?')) return; consoleStatus.textContent = 'Revoking access and stopping the terminal…'; try { await post('/api/admin/emergency-console/disable', { confirmed: true }); } catch (error) { consoleStatus.textContent = error.message || 'Console disable did not fully complete.'; } await refreshConsole(); });
    startButton?.addEventListener('click', async () => { if (!acknowledgement.checked || !window.confirm('Open a root shell on this VPS for up to 20 minutes?')) return; startButton.disabled = true; consoleStatus.textContent = 'Starting isolated terminal…'; try { await post('/api/admin/emergency-console/session', { acknowledge_root_shell: true }); window.location.assign('/emergency-terminal/'); } catch (error) { consoleStatus.textContent = error.message || 'Could not start the terminal.'; await refreshConsole(); } });
    endButton?.addEventListener('click', async () => { if (!window.confirm('Revoke the console grant and stop the terminal now?')) return; endButton.disabled = true; consoleStatus.textContent = 'Revoking access and stopping the terminal…'; try { await post('/api/admin/emergency-console/session/end', { confirmed: true }); } catch (error) { consoleStatus.textContent = error.message || 'The grant was revoked, but terminal stop was not confirmed.'; } await refreshConsole(); });
    consoleRefresh?.addEventListener('click', refreshConsole); refreshConsole();
  }
})();
"#;

pub fn router() -> Router {
    Router::new()
        .route("/admin", get(landing))
        .route("/admin/", get(landing))
        .route("/admin/operations", get(operations))
        .route("/admin/approvals", get(approvals))
        .route("/admin/deployments", get(deployments))
        .route("/admin/ssh-access", get(temporary_access))
        .route("/admin/hosting", get(hosting))
        .route("/admin/servers", get(server_fleet_page))
        .route("/admin/history", get(history))
        .route("/admin/recovery", get(recovery_page))
        .route("/admin/emergency-console", get(emergency_console_page))
        .route("/api/admin/overview", get(overview))
        .route(
            "/api/admin/servers",
            get(crate::server_fleet::admin_snapshot),
        )
        .merge(crate::node_fleet::router())
        .route("/api/admin/platform-healthz", get(platform_healthz))
        .route("/api/admin/operations", get(operations_status))
        .merge(crate::recovery::router())
        .merge(crate::emergency_console::router())
        .merge(crate::operation_history::router())
        .merge(crate::server_operations::router())
        .route("/admin/assets/admin.css", get(stylesheet))
        .route("/admin/assets/admin.js", get(script))
        .merge(crate::deploy_requests::router())
}

async fn platform_healthz() -> &'static str {
    "ok\n"
}

/// Require a root Identity session before serving any Platform admin page,
/// navigation, stylesheet, or script, then compose authorized page fragments.
/// Owner services still authorize their own records and actions.
pub async fn shell_middleware(request: Request, next: Next) -> Response {
    let path = request.uri().path().to_owned();
    let method = request.method().clone();
    if path == "/admin" || path.starts_with("/admin/") {
        if let Err(failure) = authorize_admin_shell(request.headers()).await {
            return admin_shell_auth_response(failure);
        }
    }
    let response = next.run(request).await;
    apply_admin_shell(path, method, response).await
}

#[cfg(test)]
pub async fn shell_composition_only_for_tests(request: Request, next: Next) -> Response {
    let path = request.uri().path().to_owned();
    let method = request.method().clone();
    let response = next.run(request).await;
    apply_admin_shell(path, method, response).await
}

async fn authorize_admin_shell(
    headers: &axum::http::HeaderMap,
) -> Result<(), crate::admin_auth::AuthFailure> {
    let endpoint = std::env::var("VAPOR_PLATFORM_IDENTITY_AUTHZ_URL")
        .unwrap_or_else(|_| DEFAULT_IDENTITY_AUTHZ_URL.to_owned());
    let service_token = std::env::var("VAPOR_PLATFORM_IDENTITY_AUTHZ_TOKEN").ok();
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(1))
        .timeout(Duration::from_secs(3))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| crate::admin_auth::AuthFailure::Unavailable)?;
    crate::admin_auth::authorize_root_with_client(
        headers,
        service_token.as_deref(),
        &endpoint,
        &client,
    )
    .await
}

fn admin_shell_auth_response(failure: crate::admin_auth::AuthFailure) -> Response {
    let (title, message, login_link) = match failure {
        crate::admin_auth::AuthFailure::Unauthorized => (
            "Operator sign-in required",
            "A normal Vapor account sign-in does not open the Platform admin session.",
            "<p><a href=\"/operator/login\">Continue to operator sign-in</a></p>",
        ),
        crate::admin_auth::AuthFailure::Forbidden => (
            "Access unavailable",
            "This page is not available to this account.",
            "",
        ),
        crate::admin_auth::AuthFailure::Unavailable => (
            "Access check unavailable",
            "Authorization is temporarily unavailable. Try again shortly.",
            "",
        ),
    };
    let body = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{}</title><style>body{{margin:0;background:#10161d;color:#eef3f7;font:16px/1.55 system-ui,sans-serif}}main{{width:min(620px,calc(100% - 40px));margin:12vh auto;padding:30px;border:1px solid #34434a;border-radius:10px;background:#151f2a}}a{{color:#a9d8cb}}nav{{display:flex;gap:18px;margin-top:24px}}.eyebrow{{color:#9fe1ce;font-size:12px;letter-spacing:.12em;text-transform:uppercase}}</style></head><body><main><p class=\"eyebrow\"><a href=\"/\">Vapor</a> · Access</p><h1>{}</h1><p>{}</p>{}<nav aria-label=\"Other Vapor workspaces\"><a href=\"/\">Public site</a><a href=\"/dashboard\">Your Projects</a></nav></main></body></html>",
        escape_html(title),
        escape_html(title),
        escape_html(message),
        login_link,
    );
    let mut response = (failure.status(), body).into_response();
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response
        .headers_mut()
        .insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    response
}

async fn apply_admin_shell(
    path: String,
    method: axum::http::Method,
    mut response: Response,
) -> Response {
    let is_admin_document = method == axum::http::Method::GET
        && (path == "/admin" || path.starts_with("/admin/"))
        && !path.starts_with("/admin/assets/");
    let is_admin_fragment = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim() == ADMIN_PAGE_FRAGMENT_CONTENT_TYPE);
    if is_admin_document && is_admin_fragment {
        if response.status() == StatusCode::NO_CONTENT
            || response.status() == StatusCode::NOT_MODIFIED
        {
            response.headers_mut().remove(CONTENT_TYPE);
            response
                .headers_mut()
                .insert(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
            return response;
        }
        let (parts, body) = response.into_parts();
        let bytes = match to_bytes(body, MAX_ADMIN_PAGE_BYTES).await {
            Ok(bytes) => bytes,
            Err(_) => return admin_shell_error(StatusCode::BAD_GATEWAY, &path),
        };
        let Ok(fragment) = String::from_utf8(bytes.to_vec()) else {
            return admin_shell_error(StatusCode::BAD_GATEWAY, &path);
        };
        let Some(page) = render_admin_fragment_document(&fragment, &path) else {
            return admin_shell_error(StatusCode::BAD_GATEWAY, &path);
        };
        let mut response = Response::from_parts(parts, Body::from(page));
        response.headers_mut().remove(CONTENT_LENGTH);
        response.headers_mut().insert(
            CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        );
        for header in [
            "content-encoding",
            "content-range",
            "accept-ranges",
            "etag",
            "last-modified",
            "content-disposition",
        ] {
            response.headers_mut().remove(header);
        }
        response
            .headers_mut()
            .insert(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
        return response;
    }
    response
}

fn render_admin_fragment_document(fragment: &str, current_path: &str) -> Option<String> {
    let title = admin_page_title(current_path)?;
    let mut html = String::with_capacity(fragment.len() + 1800);
    html.push_str("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>");
    html.push_str(&escape_html(title));
    html.push_str(" · Vapor Admin</title><link rel=\"stylesheet\" href=\"/admin/assets/admin.css\"><script defer src=\"/admin/assets/admin.js\"></script></head><body><a class=\"skip-link\" href=\"#admin-content\">Skip to content</a><div class=\"shell\">");
    html.push_str(&render_admin_navigation(current_path));
    html.push_str("<div class=\"admin-workspace\">");
    html.push_str(&render_admin_header(current_path));
    html.push_str("<main id=\"admin-content\" tabindex=\"-1\">");
    html.push_str(fragment);
    html.push_str("</main></div></div></body></html>");
    Some(html)
}

fn admin_page_title(current_path: &str) -> Option<&'static str> {
    let normalized_path = current_path.trim_end_matches('/');
    if normalized_path == "/admin/operations" {
        return Some("Operations");
    }
    // Keep the service's canonical name in the document title. The navigation
    // label describes the operator task; it does not rename the service.
    if section_path_matches("/admin/build-cache", normalized_path) {
        return Some("Build Cache");
    }
    MODULES
        .iter()
        .filter(|section| matches!(section.state, SectionState::Implemented))
        .filter(|section| {
            section
                .path
                .is_some_and(|path| section_path_matches(path, normalized_path))
        })
        .max_by_key(|section| section.path.map_or(0, str::len))
        .map(|section| section.name)
}

fn admin_shell_error(status: StatusCode, _current_path: &str) -> Response {
    // Keep this error document independent of the shared navigation: it must
    // remain bounded even as the operator workbench grows.
    let body = "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Admin page unavailable · Vapor Admin</title><link rel=\"stylesheet\" href=\"/admin/assets/admin.css\"></head><body><main id=\"admin-content\"><p class=\"eyebrow\">Platform</p><h1>Admin page unavailable</h1><p class=\"error\">The service returned an invalid or oversized page fragment.</p><a href=\"/admin\">Return to Platform</a></main></body></html>";
    let mut response = (status, body).into_response();
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    response
}

fn render_admin_header(current_path: &str) -> String {
    let section_name = admin_page_title(current_path).unwrap_or("Administration");
    let deployment_targets = crate::deploy_requests::SERVICE_DEPLOY_TARGETS.join(",");
    format!(
        "<header class=\"admin-header\" data-deployment-targets=\"{}\"><div><p class=\"admin-section-name\">{}</p></div><div class=\"admin-statuses\"><p class=\"admin-status-item\">Access <span class=\"status-pill status-protected\" data-operator-session role=\"status\" aria-live=\"polite\">Checking…</span></p><p class=\"admin-status-item\">Services <span class=\"status-pill\" data-platform-status role=\"status\" aria-live=\"polite\">Checking…</span></p></div></header>",
        escape_html(&deployment_targets),
        escape_html(section_name),
    )
}

fn render_admin_navigation(current_path: &str) -> String {
    let mut html = String::from(
        "<nav aria-label=\"Platform administration\"><p class=\"brand\">Vapor Platform</p>",
    );
    html.push_str("<div class=\"workspace-bridges\"><p>Vapor workspaces</p><a href=\"/\">Public site</a><a href=\"/dashboard\">Your Projects</a></div>");
    let group_order = [
        "Operate",
        "Infrastructure",
        "Ecosystem",
        "Accounts",
        "Emergency",
    ];
    for group in group_order {
        let sections: Vec<_> = MODULES
            .iter()
            .filter(|section| section.group == group)
            .collect();
        if sections.is_empty() {
            continue;
        }
        let contains_current = sections.iter().any(|section| {
            section
                .path
                .is_some_and(|path| section_path_matches(path, current_path))
        });
        html.push_str(&format!(
            "<details class=\"nav-section\"{}><summary class=\"nav-group\">{}</summary>",
            if contains_current { " open" } else { "" },
            escape_html(group)
        ));
        for section in sections {
            match (section.state, section.path) {
                (SectionState::Implemented, Some(path)) => {
                    let current = if section_path_matches(path, current_path) {
                        " aria-current=\"page\""
                    } else {
                        ""
                    };
                    let services = section.service_ids.join(",");
                    html.push_str(&format!(
                        "<a href=\"{}\" title=\"{}\" data-admin-services=\"{}\"{}><span>{}</span><small class=\"nav-status status-unknown\" data-nav-status>Checking</small></a>",
                        escape_html(path),
                        escape_html(section.description),
                        escape_html(&services),
                        current,
                        escape_html(section.name)
                    ));
                }
                (SectionState::Planned, _) => html.push_str(&format!(
                    "<span class=\"nav-disabled\" aria-disabled=\"true\"><span title=\"{}\">{}</span><small class=\"nav-status status-planned\">Planned</small></span>",
                    escape_html(section.description),
                    escape_html(section.name)
                )),
                (SectionState::Implemented, None) => {}
            }
        }
        html.push_str("</details>");
    }
    html.push_str("</nav>");
    html
}

fn section_path_matches(section_path: &str, current_path: &str) -> bool {
    current_path == section_path
        || (section_path != "/admin"
            && current_path.starts_with(&format!("{}/", section_path.trim_end_matches('/'))))
        || (section_path == "/admin" && current_path == "/admin/")
}

async fn landing() -> Response {
    let fragment = "<p class=\"eyebrow\">Platform</p><h1>Overview</h1><p class=\"page-intro\">Service health and work that needs an operator decision. Sign in with an authorized Steam-linked Vapor account to load protected information.</p><section class=\"card\" aria-labelledby=\"service-overview-title\"><div class=\"request-queue-header\"><h2 id=\"service-overview-title\">Service health</h2></div><div id=\"platform-overview\" aria-live=\"polite\"><p class=\"muted\">Checking operator access…</p></div></section><section class=\"card\" aria-labelledby=\"operator-queue-title\"><div class=\"request-queue-header\"><h2 id=\"operator-queue-title\">Approvals and execution follow-up</h2><button id=\"operator-queue-refresh\" type=\"button\">Refresh</button></div><p class=\"muted\">Counts describe loaded records; “shown” means older pages remain. Awaiting approval means a decision is still needed. Execution follow-up means approval is already saved, but deployment dispatch needs attention. Active SSH grants, running deployments, and past decisions have their own work areas.</p><div id=\"operator-queue\" aria-live=\"polite\"><p class=\"muted\">Checking operator access…</p></div></section>";
    admin_page_response(StatusCode::OK, fragment)
}

async fn server_fleet_page() -> Response {
    admin_page_response(StatusCode::OK, server_fleet_page_content())
}

fn server_fleet_page_content() -> &'static str {
    concat!(
        "<p class=\"eyebrow\">Infrastructure · Operator workbench</p><div class=\"server-fleet-toolbar\"><div><h1>Servers</h1><p class=\"page-intro\">Inspect and operate the machines that run Vapor. Use named controls for routine work; the separately gated console is there for tasks the tools do not cover.</p></div><button id=\"server-fleet-refresh\" type=\"button\">Refresh</button></div>",
        "<section class=\"card\" aria-labelledby=\"server-machines-title\"><div class=\"server-fleet-toolbar\"><h2 id=\"server-machines-title\">Machines</h2><div id=\"server-fleet-summary\" role=\"status\" aria-live=\"polite\">Loading server inventory…</div></div><div id=\"server-fleet-list\" class=\"server-fleet-grid\"><p class=\"muted\">Loading machine telemetry…</p></div></section>",
        "<section class=\"card server-room\" aria-label=\"Selected server tools\"><header class=\"server-room-selected\"><div><p class=\"eyebrow\">Selected machine</p><h2 id=\"server-selected-name\">Core server</h2></div><p id=\"server-selected-state\" class=\"muted\">Waiting for inventory…</p></header><div class=\"server-room-tabs\" role=\"tablist\" aria-label=\"Server tools\"><button id=\"server-tab-overview\" type=\"button\" role=\"tab\" aria-selected=\"true\" aria-controls=\"server-panel-overview\" tabindex=\"0\" data-server-tab=\"overview\">Overview</button><button id=\"server-tab-services\" type=\"button\" role=\"tab\" aria-selected=\"false\" aria-controls=\"server-panel-services\" tabindex=\"-1\" data-server-tab=\"services\">Services</button><button id=\"server-tab-diagnostics\" type=\"button\" role=\"tab\" aria-selected=\"false\" aria-controls=\"server-panel-diagnostics\" tabindex=\"-1\" data-server-tab=\"diagnostics\">Diagnostics</button><button id=\"server-tab-access\" type=\"button\" role=\"tab\" aria-selected=\"false\" aria-controls=\"server-panel-access\" tabindex=\"-1\" data-server-tab=\"access\">SSH access</button><button id=\"server-tab-activity\" type=\"button\" role=\"tab\" aria-selected=\"false\" aria-controls=\"server-panel-activity\" tabindex=\"-1\" data-server-tab=\"activity\">Activity</button><button id=\"server-tab-console\" type=\"button\" role=\"tab\" aria-selected=\"false\" aria-controls=\"server-panel-console\" tabindex=\"-1\" data-server-tab=\"console\">Console</button><button id=\"server-tab-connections\" type=\"button\" role=\"tab\" aria-selected=\"false\" aria-controls=\"server-panel-connections\" tabindex=\"-1\" data-server-tab=\"connections\">Connections</button></div>",
        "<section id=\"server-panel-overview\" class=\"server-room-panel\" role=\"tabpanel\" aria-labelledby=\"server-tab-overview\" data-server-panel=\"overview\"><h3>Host overview</h3><p class=\"muted\">CPU, memory, load, and uptime below describe the selected machine as a whole. Service-by-service readings are not collected yet.</p><div id=\"server-selected-overview\" class=\"server-selected-overview\" role=\"status\">Select a machine to inspect its latest sample.</div></section>",
        "<section id=\"server-panel-services\" class=\"server-room-panel\" role=\"tabpanel\" aria-labelledby=\"server-tab-services\" data-server-panel=\"services\" hidden><h3>Services and modules</h3><p class=\"muted\">Choose a named module on the selected machine. Current health and process-level resource readings are not collected per machine yet. <a href=\"/admin/hosting\">View service health</a>.</p><div class=\"server-operation-controls\"><label>Machine<select id=\"server-action-target\"><option value=\"\">Loading hosts…</option></select></label></div><div id=\"server-module-actions\" class=\"server-module-actions\"><button type=\"button\" data-service=\"homepage\" data-label=\"Homepage\">Restart Homepage</button><button type=\"button\" data-service=\"docs\" data-label=\"Docs\">Restart Docs</button><button type=\"button\" data-service=\"identity\" data-label=\"Identity\">Restart Identity</button><button type=\"button\" data-service=\"diagnostics\" data-label=\"Diagnostics\">Restart Diagnostics</button><button type=\"button\" data-service=\"registry\" data-label=\"Registry\">Restart Registry</button><button type=\"button\" data-service=\"artifact\" data-label=\"Artifact\">Restart Artifact</button><button type=\"button\" data-service=\"build_cache\" data-label=\"Build Cache\">Restart Build Cache</button><button type=\"button\" data-service=\"caddy\" data-label=\"Web gateway\">Restart Web gateway</button></div><p class=\"muted\">Restarting the machine or web gateway is currently limited to the core server.</p><button id=\"server-action-reboot\" type=\"button\">Restart machine…</button><p id=\"server-action-status\" role=\"status\" aria-live=\"polite\">Waiting for server inventory…</p></section>",
        "<section id=\"server-panel-diagnostics\" class=\"server-room-panel\" role=\"tabpanel\" aria-labelledby=\"server-tab-diagnostics\" data-server-panel=\"diagnostics\" hidden><div class=\"server-network-toolbar\"><h3>Network traffic</h3><label>History<select id=\"server-network-range\"><option value=\"1h\">1 hour</option><option value=\"6h\">6 hours</option><option value=\"24h\">24 hours</option><option value=\"7d\">7 days</option></select></label><button id=\"server-network-refresh\" type=\"button\">Refresh</button></div><p id=\"server-network-status\" class=\"muted\" role=\"status\" aria-live=\"polite\">Choose a server to inspect network counters.</p><canvas id=\"server-network-chart\" class=\"server-network-chart\" height=\"120\" aria-label=\"Inbound traffic history chart\"></canvas><div id=\"server-network-details\"></div><p class=\"muted\">Traffic history excludes loopback from totals. The agent reports interface counters and TCP state counts; it does not retain peer addresses or packet contents.</p></section>",
        "<section id=\"server-panel-access\" class=\"server-room-panel\" role=\"tabpanel\" aria-labelledby=\"server-tab-access\" data-server-panel=\"access\" hidden><h3>SSH authorizations</h3><p class=\"muted\">Review temporary SSH requests and active grants for the core server. Remote-host SSH management is not connected to this workbench yet.</p><a class=\"button-link\" href=\"/admin/ssh-access\">Open SSH access manager</a></section>",
        "<section id=\"server-panel-activity\" class=\"server-room-panel\" role=\"tabpanel\" aria-labelledby=\"server-tab-activity\" data-server-panel=\"activity\" hidden><h3>Recent server actions</h3><p class=\"muted\">Every action is recorded before it is sent to a machine. An uncertain result is shown for review and is never silently repeated.</p><div id=\"server-operation-list\" class=\"server-operation-list\"><p class=\"muted\">Loading action history…</p></div></section>",
        "<section id=\"server-panel-console\" class=\"server-room-panel\" role=\"tabpanel\" aria-labelledby=\"server-tab-console\" data-server-panel=\"console\" hidden><h3>Emergency console</h3><p class=\"muted\">This is a separate, time-limited root shell for the core server. It is not shared desktop access; typed server tools remain the normal path.</p><a class=\"button-link\" href=\"/admin/emergency-console\">Open emergency console controls</a></section>",
        "<section id=\"server-panel-connections\" class=\"server-room-panel\" role=\"tabpanel\" aria-labelledby=\"server-tab-connections\" data-server-panel=\"connections\" hidden><h3>Connect a machine</h3><p class=\"muted\">Provision the VPS through your provider first. Vapor then records the machine and prepares its outbound monitoring and typed-action connection.</p><div class=\"server-room-actions\"><button id=\"server-node-start-new\" type=\"button\">Add server<small>Connect a new machine you already own</small></button><button id=\"server-node-start-replacement\" type=\"button\">Reconnect replacement<small>Attach a fresh install to a vacant server place</small></button></div><form id=\"server-node-connect-form\" class=\"server-node-connect-form server-node-form\" hidden><h4 id=\"server-node-form-title\">Add server</h4><p id=\"server-node-form-copy\" class=\"muted\">This prepares Vapor’s connection; it does not provision hardware.</p><label>Server place<select id=\"server-node-slot\" name=\"slot_id\"><option value=\"\">Create a new place</option></select></label><label>Display name<input id=\"server-node-label\" name=\"label\" maxlength=\"80\" required autocomplete=\"off\"></label><label>Primary use<select id=\"server-node-role\" name=\"role\"><option value=\"builder\">Build worker</option><option value=\"backup\">Backup node</option><option value=\"general\">General</option><option value=\"control-plane\">Control plane</option></select></label><button id=\"server-node-submit\" type=\"submit\">Prepare connection</button><button id=\"server-node-cancel\" type=\"button\">Cancel</button></form><div id=\"server-node-secret\" class=\"server-node-secret\" hidden><h4>Connection prepared</h4><ol class=\"server-node-setup-steps\"><li>Download the config file.</li><li>Install the tested Vapor node-agent package on that machine.</li><li>Start the agent and return here; it appears online after it checks in.</li></ol><p class=\"muted\">The config contains two one-time credentials. Platform stores only their separate hashes. Keep the download private and remove extra copies.</p><pre id=\"server-node-secret-value\" hidden></pre><div class=\"server-fleet-toolbar\"><button id=\"server-node-secret-download\" type=\"button\">Download agent config</button><button id=\"server-node-secret-reveal\" type=\"button\" aria-expanded=\"false\">Show one-time credentials</button><button id=\"server-node-secret-close\" type=\"button\">Clear setup data</button></div></div><details class=\"server-node-records\"><summary>Connection records and history</summary><p id=\"server-node-management-status\" role=\"status\" aria-live=\"polite\">Loading server records…</p><div id=\"server-node-management\"></div></details></section>",
        "</section>"
    )
}

async fn operations(
    headers: axum::http::HeaderMap,
    Query(query): Query<OperationsQuery>,
) -> Response {
    let Some(view) = OperationsView::parse(query.view.as_deref()) else {
        return admin_page_response(
            StatusCode::BAD_REQUEST,
            operations_error_page(
                "Unknown Operations view",
                "Choose Approvals, Deployments, Hosting, Temporary Access, or History.",
            ),
        );
    };
    operations_view(headers, view).await
}

async fn approvals(headers: axum::http::HeaderMap) -> Response {
    operations_view(headers, OperationsView::Approvals).await
}

async fn deployments(headers: axum::http::HeaderMap) -> Response {
    operations_view(headers, OperationsView::Deployments).await
}

async fn temporary_access(headers: axum::http::HeaderMap) -> Response {
    operations_view(headers, OperationsView::TemporaryAccess).await
}

async fn hosting(headers: axum::http::HeaderMap) -> Response {
    operations_view(headers, OperationsView::Hosting).await
}

async fn history(headers: axum::http::HeaderMap) -> Response {
    operations_view(headers, OperationsView::History).await
}

async fn recovery_page(headers: axum::http::HeaderMap) -> Response {
    let endpoint = std::env::var("VAPOR_PLATFORM_IDENTITY_AUTHZ_URL")
        .unwrap_or_else(|_| DEFAULT_IDENTITY_AUTHZ_URL.to_owned());
    let service_token = std::env::var("VAPOR_PLATFORM_IDENTITY_AUTHZ_TOKEN").ok();
    let client = match reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(1))
        .timeout(Duration::from_secs(3))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client) => client,
        Err(_) => return admin_page_auth_response(crate::admin_auth::AuthFailure::Unavailable),
    };
    if let Err(failure) = crate::admin_auth::authorize_root_with_client(
        &headers,
        service_token.as_deref(),
        &endpoint,
        &client,
    )
    .await
    {
        return admin_page_auth_response(failure);
    }
    let fragment = concat!(
        "<p class=\"eyebrow\">Owner tools · Recovery</p><h1>Recovery</h1>",
        "<section class=\"card\"><div class=\"request-queue-header\"><div><h2>VPS recovery snapshot</h2><p class=\"muted\">Create a fresh validated state-only snapshot, or review the latest automatic snapshot and owner USB report. Restore and host rebuild remain separate operations.</p></div><div class=\"recovery-actions\"><button id=\"recovery-snapshot\" type=\"button\">Create snapshot now</button><button id=\"recovery-refresh\" type=\"button\">Refresh</button></div></div>",
        "<p id=\"recovery-feedback\" role=\"status\" aria-live=\"polite\">Loading recovery status…</p><dl id=\"recovery-facts\" class=\"recovery-facts\"></dl></section>",
        "<section class=\"card\"><h2>Restore readiness</h2><p><strong>Fresh-host restore drill:</strong> Not recorded.</p><p class=\"muted\">A recent server archive or owner-reported USB sync does not prove that a clean-host restore has succeeded. The USB status is a report from the trusted owner task, not independent proof of USB possession.</p><a href=\"/docs\">Open recovery runbooks</a></section>"
    );
    admin_page_response(StatusCode::OK, fragment)
}

async fn emergency_console_page(headers: axum::http::HeaderMap) -> Response {
    let endpoint = std::env::var("VAPOR_PLATFORM_IDENTITY_AUTHZ_URL")
        .unwrap_or_else(|_| DEFAULT_IDENTITY_AUTHZ_URL.to_owned());
    let service_token = std::env::var("VAPOR_PLATFORM_IDENTITY_AUTHZ_TOKEN").ok();
    let client = match reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(1))
        .timeout(Duration::from_secs(3))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client) => client,
        Err(_) => return admin_page_auth_response(crate::admin_auth::AuthFailure::Unavailable),
    };
    if let Err(failure) = crate::admin_auth::authorize_root_with_client(
        &headers,
        service_token.as_deref(),
        &endpoint,
        &client,
    )
    .await
    {
        return admin_page_auth_response(failure);
    }
    let fragment = concat!(
        "<p class=\"eyebrow\">Emergency · Explicit access</p><h1>Browser console</h1>",
        "<section class=\"card emergency-console-warning\"><h2>Full VPS root shell</h2><p>This is equivalent to root SSH. Commands can change or destroy the whole host. Use it only for a short maintenance task; Strato VNC and SSH remain the recovery path.</p><p><strong>Authentication limit:</strong> this requires a root Identity session and a separate session start, but Identity does not currently report recent password entry or a second-factor check. It is not a second factor.</p><p><strong>Origin limit:</strong> the admin dashboard currently shares a web origin with the public services. This does not isolate the root session from a script-injection bug elsewhere on that origin. Keep the host switch off until the admin origin is isolated or Identity supplies a verified step-up signal.</p><p>Vapor records session start, connection, end, expiry, and stop failures. It does not store terminal input or output; your browser memory and clipboard may still contain commands or secrets.</p></section>",
        "<section class=\"card\"><div class=\"request-queue-header\"><div><h2>Session control</h2><p id=\"emergency-console-status\" role=\"status\" aria-live=\"polite\">Loading console status…</p></div><button id=\"emergency-console-refresh\" type=\"button\">Refresh</button></div>",
        "<p><label><input id=\"emergency-console-ack\" type=\"checkbox\"> I understand this opens a root shell on the VPS for up to 20 minutes.</label></p><div class=\"form-actions\"><button id=\"emergency-console-enable\" type=\"button\" hidden>Enable console sessions</button><button id=\"emergency-console-disable\" type=\"button\" hidden>Disable and stop console</button><button id=\"emergency-console-start\" type=\"button\" disabled>Start 20-minute root session</button><button id=\"emergency-console-end\" type=\"button\" hidden>End session now</button></div><p class=\"muted\">Starting opens one terminal connection. Ending revokes access first, then stops the isolated terminal service.</p></section>",
        "<section class=\"card\"><h2>Recent access events</h2><ol id=\"emergency-console-events\" aria-live=\"polite\"><li>Loading…</li></ol></section>"
    );
    admin_page_response(StatusCode::OK, fragment)
}

async fn operations_view(headers: axum::http::HeaderMap, view: OperationsView) -> Response {
    let endpoint = std::env::var("VAPOR_PLATFORM_IDENTITY_AUTHZ_URL")
        .unwrap_or_else(|_| DEFAULT_IDENTITY_AUTHZ_URL.to_owned());
    let service_token = std::env::var("VAPOR_PLATFORM_IDENTITY_AUTHZ_TOKEN").ok();
    let client = match reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(1))
        .timeout(Duration::from_secs(3))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client) => client,
        Err(_) => return admin_page_auth_response(crate::admin_auth::AuthFailure::Unavailable),
    };
    if let Err(failure) = crate::admin_auth::authorize_root_with_client(
        &headers,
        service_token.as_deref(),
        &endpoint,
        &client,
    )
    .await
    {
        return admin_page_auth_response(failure);
    }
    admin_page_response(StatusCode::OK, operations_page(view))
}

fn approvals_page_content() -> &'static str {
    concat!(
        "<section class=\"card\"><div class=\"request-queue-header\"><div><h2>Decision inbox</h2><p class=\"muted\">Only unanswered requests appear here. Each decision is recorded by the service that owns it; deployment execution is tracked separately.</p></div><button id=\"approvals-refresh\" type=\"button\">Refresh</button></div><p id=\"approvals-feedback\" role=\"status\" aria-live=\"polite\"></p></section>",
        "<details class=\"approval-source\" open><summary>Deployments <span id=\"pending-deploy-count\" class=\"approval-source-count\">Loading…</span></summary><div class=\"approval-source-body\"><div id=\"pending-deploy-approvals\" aria-live=\"polite\"><p class=\"muted\">Loading deployment approvals…</p></div><button id=\"approvals-more-deploy\" type=\"button\" hidden>Load older requests</button></div></details>",
        "<details class=\"approval-source\"><summary>SSH access <span id=\"pending-ssh-count\" class=\"approval-source-count\">Loading…</span></summary><div class=\"approval-source-body\"><div id=\"pending-ssh-approvals\" aria-live=\"polite\"><p class=\"muted\">Loading SSH access requests…</p></div><button id=\"approvals-more-ssh\" type=\"button\" hidden>Load older requests</button></div></details>",
        "<details class=\"approval-source\"><summary>Identity permissions <span id=\"pending-identity-count\" class=\"approval-source-count\">Loading…</span></summary><div class=\"approval-source-body\"><div id=\"pending-identity-approvals\" aria-live=\"polite\"><p class=\"muted\">Loading Identity permission requests…</p></div><button id=\"approvals-more-identity\" type=\"button\" hidden>Load older requests</button></div></details>"
    )
}

fn operations_page(view: OperationsView) -> String {
    let content = match view {
        OperationsView::Approvals => approvals_page_content().to_owned(),
        OperationsView::Deployments => concat!(
            "<section class=\"card\"><div class=\"request-queue-header\"><div><h2>Deployment queue</h2><p class=\"muted\">Queued or running work is shown separately from dispatch failures where no worker started. Superseded revisions cannot be retried. New decisions are made in <a href=\"/admin/approvals\">Approvals</a>; closed and completed records are in <a href=\"/admin/history\">History</a>.</p></div><button id=\"deployment-refresh\" type=\"button\">Refresh</button></div><div id=\"deploy-requests\" aria-live=\"polite\"><p class=\"muted\">Loading deployment queue…</p></div><button id=\"deployments-more\" type=\"button\" hidden>Load older records</button></section>",
            "<section class=\"card\"><h2>Latest deploy or restore result</h2><p class=\"muted\">This is the current Platform status pointer. Historical requests are listed separately under History.</p><div id=\"latest-operation-status\" aria-live=\"polite\"><p class=\"muted\">Loading latest operation result…</p></div><a href=\"/docs\">Open server runbooks</a></section>"
        )
        .to_owned(),
        OperationsView::Hosting => "<section class=\"card\"><h2>Service health and feature readiness</h2><p class=\"muted\">Health reports whether a service answers its probe. Feature readiness is shown only when the service reports it; “not reported” is not treated as ready.</p><div id=\"operations-status\" aria-live=\"polite\"><p class=\"muted\">Loading service status…</p></div></section><section class=\"card\"><h2>Runbooks</h2><p class=\"muted\">Use the service-owned recovery guidance for operational changes.</p><a href=\"/docs\">Open server runbooks</a></section>".to_owned(),
        OperationsView::TemporaryAccess => "<p class=\"eyebrow\">Operate · Access</p><div class=\"request-queue-header\"><div><h1>SSH access</h1><p class=\"muted\">Manage active grants and closed access records. Pending requests are reviewed in <a href=\"/admin/approvals#pending-ssh-approvals\">Approvals</a>.</p></div><button id=\"ssh-access-refresh\" type=\"button\">Refresh</button></div><section class=\"ssh-workbench\" aria-label=\"SSH access workbench\"><div class=\"ssh-list-panel\"><div class=\"ssh-list-toolbar\"><label for=\"ssh-access-search\">Find a grant<input id=\"ssh-access-search\" type=\"search\" placeholder=\"Purpose, fingerprint, account\" autocomplete=\"off\"></label><label for=\"ssh-access-filter\">Show<select id=\"ssh-access-filter\"><option value=\"active\">Active grants</option><option value=\"all\">All grants</option><option value=\"history\">Closed grants</option></select></label><p id=\"ssh-access-count\" class=\"ssh-list-count\" aria-live=\"polite\">Loading grants…</p></div><ul id=\"ssh-access-list\" class=\"ssh-record-list\" aria-label=\"SSH grants\"><li class=\"muted\">Loading…</li></ul><button id=\"ssh-access-more\" type=\"button\" hidden>Load older records</button></div><section id=\"ssh-access-detail\" class=\"ssh-detail-panel\" aria-live=\"polite\"><p class=\"muted\">Choose a grant to inspect its details.</p></section></section>".to_owned(),
        OperationsView::History => concat!(
            "<p class=\"eyebrow\">Platform · Records</p><h1>History</h1>",
            "<section class=\"card\"><p class=\"muted\">Search records from their owning services. Select a row to inspect its details.</p>",
            "<div class=\"history-toolbar\"><p id=\"history-count\" class=\"history-count muted\"></p><div><button id=\"history-load-more\" type=\"button\" hidden>Load older records</button> <button id=\"history-refresh\" type=\"button\">Refresh</button></div></div>",
            "<div class=\"history-controls\"><label for=\"history-search\">Search<input id=\"history-search\" type=\"search\" autocomplete=\"off\" placeholder=\"Name, revision, ID, fingerprint…\"></label><label for=\"history-kind\">Area<select id=\"history-kind\"><option value=\"all\">All areas</option><option>Platform operation</option><option>Deployment</option><option>Temporary access</option><option>Identity request</option></select></label><label for=\"history-status\">Status<select id=\"history-status\"><option value=\"all\">Any status</option><option value=\"attention\">Needs attention</option><option value=\"active\">In progress</option><option value=\"complete\">Completed</option></select></label></div>",
            "<details class=\"history-source-panel\"><summary>Record sources</summary><div id=\"history-sources\" class=\"history-source-status\" aria-live=\"polite\"><span>Loading sources…</span></div></details>",
            "<div id=\"operations-history\" aria-live=\"polite\"><p class=\"muted\">Loading records…</p></div></section>"
        )
        .to_owned(),
    };
    if view == OperationsView::TemporaryAccess {
        return content;
    }
    format!(
        "<p class=\"eyebrow\">Platform · {}</p><h1>{}</h1>{content}",
        view.area(),
        view.name(),
        content = content
    )
}

fn operations_error_page(title: &str, message: &str) -> String {
    format!(
        "<p class=\"eyebrow\">Platform · {}</p><h1>Operations</h1><p class=\"error\">{}</p><a href=\"/admin/approvals\">Return to Approvals</a>",
        escape_html(title),
        escape_html(message)
    )
}

#[derive(Serialize)]
struct Overview {
    state: &'static str,
    deployed_revision: &'static str,
    services: Vec<ServiceSummary>,
}

#[derive(Serialize)]
struct ServiceSummary {
    id: &'static str,
    name: &'static str,
    health: &'static str,
    readiness: String,
    admin_url: Option<&'static str>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OperationRecord {
    schema_version: u32,
    operation: String,
    result: String,
    started_at_utc: String,
    finished_at_utc: String,
    revision: String,
    server_mode: String,
}

#[derive(Serialize)]
struct OperationStatus {
    record_status: &'static str,
    record: Option<OperationRecord>,
}

#[derive(Deserialize)]
struct OperationsQuery {
    view: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OperationsView {
    Approvals,
    Deployments,
    Hosting,
    TemporaryAccess,
    History,
}

impl OperationsView {
    fn parse(value: Option<&str>) -> Option<Self> {
        match value.unwrap_or("approvals") {
            "approvals" => Some(Self::Approvals),
            "deployments" => Some(Self::Deployments),
            "hosting" => Some(Self::Hosting),
            "temporary-access" => Some(Self::TemporaryAccess),
            "history" => Some(Self::History),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Approvals => "Approvals",
            Self::Deployments => "Deployments",
            Self::Hosting => "Hosting",
            Self::TemporaryAccess => "SSH Access",
            Self::History => "History",
        }
    }

    fn area(self) -> &'static str {
        match self {
            Self::Approvals | Self::Deployments | Self::Hosting | Self::TemporaryAccess => {
                "Operate"
            }
            Self::History => "Records",
        }
    }
}

async fn overview(headers: axum::http::HeaderMap) -> Response {
    let endpoint = std::env::var("VAPOR_PLATFORM_IDENTITY_AUTHZ_URL")
        .unwrap_or_else(|_| DEFAULT_IDENTITY_AUTHZ_URL.to_owned());
    let service_token = std::env::var("VAPOR_PLATFORM_IDENTITY_AUTHZ_TOKEN").ok();
    let origin = std::env::var("VAPOR_PLATFORM_PROBE_ORIGIN")
        .unwrap_or_else(|_| DEFAULT_PROBE_ORIGIN.to_owned());
    let auth_client = match reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(1))
        .timeout(Duration::from_secs(3))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client) => client,
        Err(_) => return auth_unavailable_response(crate::admin_auth::AuthFailure::Unavailable),
    };
    let probe_client = match reqwest::Client::builder()
        .connect_timeout(Duration::from_millis(500))
        .timeout(Duration::from_secs(2))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client) => client,
        Err(_) => {
            return private_json_response(
                StatusCode::SERVICE_UNAVAILABLE,
                serde_json::json!({"error":"service_probe_unavailable"}),
            );
        }
    };
    overview_with_clients(
        &headers,
        service_token.as_deref(),
        &endpoint,
        &origin,
        &auth_client,
        &probe_client,
    )
    .await
}

async fn operations_status(headers: axum::http::HeaderMap) -> Response {
    let endpoint = std::env::var("VAPOR_PLATFORM_IDENTITY_AUTHZ_URL")
        .unwrap_or_else(|_| DEFAULT_IDENTITY_AUTHZ_URL.to_owned());
    let service_token = std::env::var("VAPOR_PLATFORM_IDENTITY_AUTHZ_TOKEN").ok();
    let record_path = std::env::var("VAPOR_PLATFORM_OPERATIONS_FILE")
        .unwrap_or_else(|_| "/var/lib/vapor-server/operations/latest.json".to_owned());
    let client = match reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(1))
        .timeout(Duration::from_secs(3))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client) => client,
        Err(_) => return auth_unavailable_response(crate::admin_auth::AuthFailure::Unavailable),
    };
    operations_status_with_client(
        &headers,
        service_token.as_deref(),
        &endpoint,
        &client,
        Path::new(&record_path),
    )
    .await
}

async fn operations_status_with_client(
    headers: &axum::http::HeaderMap,
    service_token: Option<&str>,
    endpoint: &str,
    client: &reqwest::Client,
    record_path: &Path,
) -> Response {
    if let Err(failure) =
        crate::admin_auth::authorize_root_with_client(headers, service_token, endpoint, client)
            .await
    {
        return auth_unavailable_response(failure);
    }
    let (record_status, record) = match read_operation_record(record_path).await {
        Ok(Some(record)) => ("reported", Some(record)),
        Ok(None) => ("not_reported", None),
        Err(()) => ("invalid", None),
    };
    private_json_response(
        StatusCode::OK,
        OperationStatus {
            record_status,
            record,
        },
    )
}

async fn read_operation_record(path: &Path) -> Result<Option<OperationRecord>, ()> {
    const MAX_RECORD_BYTES: usize = 4096;
    let file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(()),
    };
    let mut bytes = Vec::with_capacity(MAX_RECORD_BYTES);
    file.take((MAX_RECORD_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| ())?;
    if bytes.is_empty() || bytes.len() > MAX_RECORD_BYTES {
        return Err(());
    }
    let record: OperationRecord = serde_json::from_slice(&bytes).map_err(|_| ())?;
    let valid = record.schema_version == 1
        && matches!(record.operation.as_str(), "deploy" | "restore")
        && matches!(record.result.as_str(), "success" | "failed")
        && valid_utc_timestamp(&record.started_at_utc)
        && valid_utc_timestamp(&record.finished_at_utc)
        && record.finished_at_utc >= record.started_at_utc
        && (record.revision == "unknown"
            || ((7..=40).contains(&record.revision.len())
                && record.revision.bytes().all(|byte| byte.is_ascii_hexdigit())))
        && matches!(record.server_mode.as_str(), "monolith" | "services");
    if valid {
        Ok(Some(record))
    } else {
        Err(())
    }
}

fn valid_utc_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 20 {
        return false;
    }
    if bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
    {
        return false;
    }
    if [0..4, 5..7, 8..10, 11..13, 14..16, 17..19]
        .into_iter()
        .any(|range| !bytes[range].iter().all(u8::is_ascii_digit))
    {
        return false;
    }
    let number = |range: std::ops::Range<usize>| value[range].parse::<u32>().ok();
    let (Some(year), Some(month), Some(day), Some(hour), Some(minute), Some(second)) = (
        number(0..4),
        number(5..7),
        number(8..10),
        number(11..13),
        number(14..16),
        number(17..19),
    ) else {
        return false;
    };
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 400 == 0 || (year % 4 == 0 && year % 100 != 0) => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days_in_month).contains(&day) && hour <= 23 && minute <= 59 && second <= 59
}

async fn overview_with_clients(
    headers: &axum::http::HeaderMap,
    service_token: Option<&str>,
    endpoint: &str,
    origin: &str,
    auth_client: &reqwest::Client,
    probe_client: &reqwest::Client,
) -> Response {
    if let Err(failure) =
        crate::admin_auth::authorize_root_with_client(headers, service_token, endpoint, auth_client)
            .await
    {
        return auth_unavailable_response(failure);
    }
    if !valid_probe_origin(origin) {
        return private_json_response(
            StatusCode::SERVICE_UNAVAILABLE,
            serde_json::json!({"error":"service_probe_unavailable"}),
        );
    }

    let services = probe_services(probe_client, origin).await;
    private_json_response(
        StatusCode::OK,
        Overview {
            state: "serving",
            deployed_revision: option_env!("VAPOR_PLATFORM_REVISION").unwrap_or("local build"),
            services,
        },
    )
}

fn auth_unavailable_response(failure: crate::admin_auth::AuthFailure) -> Response {
    private_json_response(
        failure.status(),
        serde_json::json!({"error":match failure {
            crate::admin_auth::AuthFailure::Unauthorized => "sign_in_required",
            crate::admin_auth::AuthFailure::Forbidden => "root_required",
            crate::admin_auth::AuthFailure::Unavailable => "identity_authorization_unavailable",
        }}),
    )
}

fn admin_page_auth_response(failure: crate::admin_auth::AuthFailure) -> Response {
    let fragment = match failure {
        crate::admin_auth::AuthFailure::Unauthorized => {
            "<p class=\"eyebrow\">Platform · Operator access</p><h1>Operator sign-in required</h1><p>A normal Vapor account sign-in does not open the Platform admin session. Use the separate operator sign-in; admin access still requires the Platform root role.</p><a class=\"button-link\" href=\"/operator/login\">Continue to operator sign-in</a>"
        }
        crate::admin_auth::AuthFailure::Forbidden => {
            "<p class=\"eyebrow\">Platform · Operator access</p><h1>Root access required</h1><p>You are signed in with an operator session, but this account does not have the Platform root role.</p><a href=\"/operator/login\">View operator account</a>"
        }
        crate::admin_auth::AuthFailure::Unavailable => {
            "<p class=\"eyebrow\">Platform · Operations</p><h1>Authorization unavailable</h1><p>Identity authorization is temporarily unavailable. Retry in a moment.</p>"
        }
    };
    admin_page_response(failure.status(), fragment)
}

async fn probe_services(client: &reqwest::Client, origin: &str) -> Vec<ServiceSummary> {
    probe_services_for_mode(
        client,
        origin,
        std::env::var("VAPOR_SERVER_MODE").as_deref() == Ok("services"),
    )
    .await
}

async fn probe_services_for_mode(
    client: &reqwest::Client,
    origin: &str,
    services_mode: bool,
) -> Vec<ServiceSummary> {
    if services_mode {
        let (
            platform,
            homepage,
            docs,
            identity,
            diagnostics,
            registry,
            artifacts,
            build_cache,
            build_cache_readiness,
        ) = tokio::join!(
            probe_health(client, "http://127.0.0.1:7110", "/healthz"),
            probe_health(client, "http://127.0.0.1:7111", "/healthz"),
            probe_health(client, "http://127.0.0.1:7112", "/healthz"),
            probe_health(client, "http://127.0.0.1:7113", "/healthz"),
            probe_health(client, "http://127.0.0.1:7114", "/healthz"),
            probe_health(client, "http://127.0.0.1:7115", "/healthz"),
            probe_health(client, "http://127.0.0.1:7116", "/healthz"),
            probe_health(client, "http://127.0.0.1:7117", "/healthz"),
            probe_build_cache_readiness(client, "http://127.0.0.1:7117", "/v1/status"),
        );
        return vec![
            service(
                "platform",
                "Platform",
                platform,
                "not reported",
                Some("/admin"),
            ),
            service("homepage", "Homepage", homepage, "not reported", None),
            service("docs", "Docs", docs, "not reported", None),
            service(
                "identity",
                "Identity",
                identity,
                "not reported",
                Some("/admin/identity"),
            ),
            service(
                "diagnostics",
                "Diagnostics",
                diagnostics,
                "not reported",
                None,
            ),
            service(
                "registry",
                "Registry",
                registry,
                "not reported",
                Some("/admin/registry"),
            ),
            service(
                "artifacts",
                "Artifacts",
                artifacts,
                "not reported",
                Some("/admin/artifacts"),
            ),
            service(
                "build-cache",
                "Build Cache",
                build_cache,
                build_cache_readiness,
                Some("/admin/build-cache"),
            ),
        ];
    }

    let (
        platform,
        homepage,
        docs,
        identity,
        diagnostics,
        registry,
        artifacts,
        build_cache,
        build_cache_readiness,
    ) = tokio::join!(
        probe_health(client, origin, "/api/admin/platform-healthz"),
        probe_health(client, origin, "/healthz"),
        probe_health(client, origin, "/docs/healthz"),
        probe_health(client, origin, "/api/identity/v1/status"),
        probe_health(client, origin, "/api/diagnostics/healthz"),
        probe_health(client, origin, "/api/registry/healthz"),
        probe_health(client, origin, "/api/updates/healthz"),
        probe_health(client, origin, "/api/build-cache/healthz"),
        probe_build_cache_readiness(client, origin, "/api/build-cache/v1/status"),
    );
    vec![
        service(
            "platform",
            "Platform",
            platform,
            "not reported",
            Some("/admin"),
        ),
        service("homepage", "Homepage", homepage, "not reported", None),
        service("docs", "Docs", docs, "not reported", None),
        service(
            "identity",
            "Identity",
            identity,
            "not reported",
            Some("/admin/identity"),
        ),
        service(
            "diagnostics",
            "Diagnostics",
            diagnostics,
            "not reported",
            None,
        ),
        service(
            "registry",
            "Registry",
            registry,
            "not reported",
            Some("/admin/registry"),
        ),
        service(
            "artifacts",
            "Artifacts",
            artifacts,
            "not reported",
            Some("/admin/artifacts"),
        ),
        service(
            "build-cache",
            "Build Cache",
            build_cache,
            build_cache_readiness,
            Some("/admin/build-cache"),
        ),
    ]
}

async fn probe_health(client: &reqwest::Client, origin: &str, path: &str) -> &'static str {
    match client.get(format!("{origin}{path}")).send().await {
        Ok(response) if response.status().is_success() => "healthy",
        _ => "unavailable",
    }
}

async fn probe_build_cache_readiness(client: &reqwest::Client, origin: &str, path: &str) -> String {
    let mut response = match client.get(format!("{origin}{path}")).send().await {
        Ok(response) if response.status().is_success() => response,
        _ => return "unavailable".to_owned(),
    };
    const MAX_STATUS_BYTES: usize = 64 * 1024;
    let mut body = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) if body.len().saturating_add(chunk.len()) <= MAX_STATUS_BYTES => {
                body.extend_from_slice(&chunk);
            }
            Ok(Some(_)) => return "unknown".to_owned(),
            Ok(None) => break,
            Err(_) => return "unavailable".to_owned(),
        }
    }
    let Ok(status) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return "unknown".to_owned();
    };
    build_cache_readiness(&status)
}

fn build_cache_readiness(status: &serde_json::Value) -> String {
    const TARGETS: [&str; 2] = ["x86_64-unknown-linux-gnu", "x86_64-pc-windows-gnullvm"];
    let Some(targets) = status.get("targets").and_then(serde_json::Value::as_array) else {
        return "unknown".to_owned();
    };

    let mut seen = [false; TARGETS.len()];
    let mut configured = 0;
    let mut current = 0;
    for target in targets {
        let Some(triple) = target
            .get("target_triple")
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        let Some(index) = TARGETS.iter().position(|supported| *supported == triple) else {
            continue;
        };
        if seen[index] {
            return "unknown".to_owned();
        }
        seen[index] = true;
        let (Some(is_configured), Some(is_current)) = (
            target
                .get("configured")
                .and_then(serde_json::Value::as_bool),
            target.get("current").and_then(serde_json::Value::as_bool),
        ) else {
            return "unknown".to_owned();
        };
        if is_current && !is_configured {
            return "unknown".to_owned();
        }
        configured += usize::from(is_configured);
        current += usize::from(is_current);
    }
    if seen.iter().any(|found| !found) {
        return "unknown".to_owned();
    }

    format!(
        "{current}/{} snapshots current; {configured}/{} targets configured",
        TARGETS.len(),
        TARGETS.len()
    )
}

fn service(
    id: &'static str,
    name: &'static str,
    health: &'static str,
    readiness: impl Into<String>,
    admin_url: Option<&'static str>,
) -> ServiceSummary {
    ServiceSummary {
        id,
        name,
        health,
        readiness: readiness.into(),
        admin_url,
    }
}

fn valid_probe_origin(origin: &str) -> bool {
    crate::admin_auth::valid_loopback_endpoint(origin)
        && reqwest::Url::parse(origin)
            .is_ok_and(|url| url.path() == "/" && url.query().is_none() && url.fragment().is_none())
}

fn private_json_response<T: Serialize>(status: StatusCode, body: T) -> Response {
    let mut response = (status, Json(body)).into_response();
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    response
}

async fn stylesheet() -> Response {
    html_response(StatusCode::OK, "text/css; charset=utf-8", CSS.to_owned())
}

async fn script() -> Response {
    html_response(
        StatusCode::OK,
        "text/javascript; charset=utf-8",
        JS.to_owned(),
    )
}

fn html_response(status: StatusCode, content_type: &'static str, body: String) -> Response {
    let mut response = body.into_response();
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    response
}

fn admin_page_response(status: StatusCode, fragment: impl Into<String>) -> Response {
    let mut response = fragment.into().into_response();
    *response.status_mut() = status;
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/vnd.vapor.admin-page.v1+html; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    response
}

fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::overview_with_clients;
    use axum::body::Body;
    use axum::extract::State;
    use axum::http::Uri;
    use axum::http::{HeaderMap, HeaderValue, Request, StatusCode};
    use axum::response::IntoResponse;
    use axum::routing::{any, post};
    use axum::{Json, Router};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::net::TcpListener;

    const SESSION: &str =
        "vapor-session-12345678-1234-4234-9234-123456789abc-12345678-1234-4234-9234-123456789abc";
    const SERVICE_TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    // Keep shell-composition tests independent from Identity availability;
    // the production middleware's authorization gate has a dedicated test.
    fn admin_app() -> Router {
        super::router().layer(axum::middleware::from_fn(
            super::shell_composition_only_for_tests,
        ))
    }

    #[tokio::test]
    async fn admin_shell_denies_documents_and_assets_before_serving_them_without_a_session() {
        let app = super::router().layer(axum::middleware::from_fn(super::shell_middleware));

        for path in [
            "/admin",
            "/admin/approvals",
            "/admin/assets/admin.css",
            "/admin/assets/admin.js",
        ] {
            let response = app
                .clone()
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
            assert_eq!(response.headers()["cache-control"], "private, no-store");
            let body = response.into_body().collect().await.unwrap().to_bytes();
            let html = std::str::from_utf8(&body).unwrap();
            assert!(html.contains("Operator sign-in required"));
            assert!(!html.contains("Platform administration"));
            assert!(!html.contains("/admin/assets/admin.css"));
            assert!(!html.contains("/admin/assets/admin.js"));
        }
    }

    #[derive(Clone)]
    struct OverviewMockState {
        authorization_requests: Arc<AtomicUsize>,
        health_requests: Arc<AtomicUsize>,
    }

    async fn mock_authorize(
        State(state): State<OverviewMockState>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> Json<Value> {
        state.authorization_requests.fetch_add(1, Ordering::SeqCst);
        let valid = headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value == format!("Bearer {SERVICE_TOKEN}"))
            && body["audience"] == "vapor-platform-admin"
            && body["session_token"] == SESSION
            && body["policy"]["mode"] == "all"
            && body["policy"]["requirements"][0]["role"] == "root";
        Json(
            json!({"allowed": valid, "expires_at_unix": i64::MAX, "profile_id": if valid { Some("mock-root-profile") } else { None }}),
        )
    }

    async fn mock_probe(
        State(state): State<OverviewMockState>,
        uri: Uri,
    ) -> axum::response::Response {
        state.health_requests.fetch_add(1, Ordering::SeqCst);
        if uri.path() == "/api/build-cache/v1/status" {
            return Json(json!({
                "targets": [
                    {"target_triple":"x86_64-unknown-linux-gnu","configured":false,"current":false},
                    {"target_triple":"x86_64-pc-windows-gnullvm","configured":false,"current":false}
                ]
            }))
            .into_response();
        }
        let status = if uri.path() == "/docs/healthz" {
            StatusCode::SERVICE_UNAVAILABLE
        } else {
            StatusCode::OK
        };
        (status, "private-probe-payload").into_response()
    }
    use tower::ServiceExt;

    async fn fake_health(uri: Uri) -> axum::response::Response {
        if uri.path() == "/api/build-cache/v1/status" {
            return Json(json!({
                "targets": [
                    {"target_triple":"x86_64-unknown-linux-gnu","configured":false,"current":false},
                    {"target_triple":"x86_64-pc-windows-gnullvm","configured":false,"current":false}
                ]
            }))
            .into_response();
        }
        let status = if uri.path() == "/docs/healthz" {
            StatusCode::SERVICE_UNAVAILABLE
        } else {
            StatusCode::OK
        };
        (status, "private-probe-payload").into_response()
    }

    async fn fake_monolith_health(uri: Uri) -> axum::response::Response {
        let status = if uri.path() == "/api/admin/platform-healthz" {
            StatusCode::SERVICE_UNAVAILABLE
        } else {
            StatusCode::OK
        };
        (status, "private-probe-payload").into_response()
    }

    async fn slow_health() -> &'static str {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        "ok"
    }

    #[tokio::test]
    async fn admin_root_lists_fixed_modules() {
        let response = admin_app()
            .oneshot(
                Request::builder()
                    .uri("/admin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let html = std::str::from_utf8(&body).unwrap();
        for route in ["/admin", "/admin/identity"] {
            assert!(html.contains(route), "missing navigation route {route}");
        }
        for route in [
            "/admin/approvals",
            "/admin/deployments",
            "/admin/ssh-access",
            "/admin/hosting",
            "/admin/history",
        ] {
            assert!(html.contains(&format!("href=\"{route}\"")));
        }
    }

    #[tokio::test]
    async fn platform_landing_returns_the_owner_page_fragment_contract() {
        let response = super::landing().await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()["content-type"],
            "application/vnd.vapor.admin-page.v1+html; charset=utf-8"
        );
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let fragment = std::str::from_utf8(&body).unwrap();
        assert!(fragment.starts_with("<p class=\"eyebrow\">Platform</p>"));
        assert!(!fragment.contains("<!doctype html>"));
        assert!(!fragment.contains("<main"));
        assert!(fragment.contains("<h1>Overview</h1>"));
        assert!(fragment.contains("id=\"platform-overview\""));
        assert!(fragment.contains("id=\"operator-queue\""));
        assert!(fragment.contains("“shown” means older pages remain"));
        assert!(!fragment.contains("Operator work areas"));
        assert!(!fragment.contains("action-links"));
    }

    #[tokio::test]
    async fn overview_uses_compact_health_and_action_tables() {
        let response = admin_app()
            .oneshot(
                Request::builder()
                    .uri("/admin/assets/admin.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let script = std::str::from_utf8(&body).unwrap();
        assert!(script.contains("className = 'service-directory'"));
        assert!(script.contains("className = 'request-queue-table'"));
        assert!(script.contains("service.readiness !== 'not reported'"));
        assert!(script.contains("All service probes healthy"));
        assert!(script.contains("overview.id === 'platform-overview'"));
        assert!(script.contains("source.summarize(records)"));
        assert!(script.contains(
            "moreAvailable = !Array.isArray(items) && typeof items?.next_cursor === 'string'"
        ));
        assert!(script.contains("permission-requests/page?limit=200"));
        assert!(script.contains("${outcome.moreAvailable ? ' shown' : ''}"));
        assert!(script.contains("hosting.textContent = 'View service details'"));
        assert!(script.contains("actionHref = '/admin/deployments'"));
        assert!(script.contains("actionHref = '/admin/ssh-access'"));
        assert!(!script.contains("className = 'service-grid'"));
        assert!(!script.contains("className = 'request-queue-grid'"));
    }

    #[tokio::test]
    async fn shared_shell_does_not_parse_uncontracted_owner_html() {
        use axum::http::header::CONTENT_TYPE;
        use axum::routing::get;

        const FULL_DOCUMENT: &str = "<!doctype html><html><body><main id=\"admin-content\"><h1>legacy</h1></main></body></html>";
        let owner = Router::new().route(
            "/admin/build-cache",
            get(|| async {
                let mut response = (StatusCode::OK, FULL_DOCUMENT).into_response();
                response.headers_mut().insert(
                    CONTENT_TYPE,
                    HeaderValue::from_static("text/html; charset=utf-8"),
                );
                response
            }),
        );
        let response = owner
            .layer(axum::middleware::from_fn(
                super::shell_composition_only_for_tests,
            ))
            .oneshot(
                Request::get("/admin/build-cache")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.headers()[CONTENT_TYPE], "text/html; charset=utf-8");
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(std::str::from_utf8(&body).unwrap(), FULL_DOCUMENT);
    }

    #[tokio::test]
    async fn shared_shell_skip_link_target_is_keyboard_focusable() {
        let document =
            super::render_admin_fragment_document("<h1>Accessible page</h1>", "/admin/operations")
                .unwrap();
        assert!(document.contains("href=\"#admin-content\">Skip to content</a>"));
        assert!(document.contains("<main id=\"admin-content\" tabindex=\"-1\">"));
    }

    #[tokio::test]
    async fn history_filters_stack_before_their_minimum_width_overflows() {
        let response = super::stylesheet().await;
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let css = std::str::from_utf8(&body).unwrap();
        assert!(css.contains(
            "@media(max-width:620px) { .history-controls { grid-template-columns:1fr; }"
        ));
        assert!(css.contains(
            "@media(max-width:760px) { .history-workbench { grid-template-columns:1fr; }"
        ));
    }

    #[tokio::test]
    async fn shared_shell_rejects_oversized_owner_fragments_with_a_bounded_error_page() {
        use axum::http::header::CONTENT_TYPE;
        use axum::routing::get;

        let oversized = vec![b'x'; super::MAX_ADMIN_PAGE_BYTES + 1];
        let owner = Router::new().route(
            "/admin/build-cache",
            get(move || {
                let body = oversized.clone();
                async move {
                    let mut response = (StatusCode::OK, Body::from(body)).into_response();
                    response.headers_mut().insert(
                        CONTENT_TYPE,
                        HeaderValue::from_static("application/vnd.vapor.admin-page.v1+html"),
                    );
                    response
                }
            }),
        );
        let response = owner
            .layer(axum::middleware::from_fn(
                super::shell_composition_only_for_tests,
            ))
            .oneshot(
                Request::get("/admin/build-cache")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert!(body.len() < 4096);
        assert!(std::str::from_utf8(&body)
            .unwrap()
            .contains("invalid or oversized page fragment"));
    }

    #[tokio::test]
    async fn shared_shell_composes_explicit_owner_page_fragment_without_html_parsing() {
        use axum::http::header::CONTENT_TYPE;
        use axum::routing::get;

        let owner = Router::new().route(
            "/admin/build-cache",
            get(|| async {
                let mut response =
                    (StatusCode::OK, "<h1>Owner-rendered snapshot table</h1>").into_response();
                response.headers_mut().insert(
                    CONTENT_TYPE,
                    HeaderValue::from_static("application/vnd.vapor.admin-page.v1+html"),
                );
                response
                    .headers_mut()
                    .insert("content-encoding", HeaderValue::from_static("gzip"));
                response
                    .headers_mut()
                    .insert("etag", HeaderValue::from_static("\"fragment-v1\""));
                response.headers_mut().insert(
                    "content-disposition",
                    HeaderValue::from_static("attachment; filename=fragment.html"),
                );
                response
            }),
        );
        let response = owner
            .layer(axum::middleware::from_fn(
                super::shell_composition_only_for_tests,
            ))
            .oneshot(
                Request::get("/admin/build-cache")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        assert!(!response.headers().contains_key("content-encoding"));
        assert!(!response.headers().contains_key("etag"));
        assert!(!response.headers().contains_key("content-disposition"));
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains("<title>Build Cache · Vapor Admin</title>"));
        assert!(html.contains("<nav aria-label=\"Platform administration\">"));
        assert!(html.contains("<main id=\"admin-content\" tabindex=\"-1\">"));
        assert!(html.contains("<h1>Owner-rendered snapshot table</h1>"));
        assert_eq!(html.matches("<body>").count(), 1);
    }

    #[tokio::test]
    async fn shared_shell_wraps_owner_fragment_errors_without_changing_status() {
        use axum::http::header::CONTENT_TYPE;
        use axum::routing::get;

        let owner = Router::new().route(
            "/admin/build-cache",
            get(|| async {
                let mut response =
                    (StatusCode::FORBIDDEN, "<p>Root access is required.</p>").into_response();
                response.headers_mut().insert(
                    CONTENT_TYPE,
                    HeaderValue::from_static("application/vnd.vapor.admin-page.v1+html"),
                );
                response
            }),
        );
        let response = owner
            .layer(axum::middleware::from_fn(
                super::shell_composition_only_for_tests,
            ))
            .oneshot(
                Request::get("/admin/build-cache")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert!(std::str::from_utf8(&body)
            .unwrap()
            .contains("Root access is required."));
    }

    #[tokio::test]
    async fn shared_shell_does_not_add_a_document_body_to_no_content_responses() {
        use axum::http::header::CONTENT_TYPE;
        use axum::routing::get;

        let owner = Router::new().route(
            "/admin/build-cache",
            get(|| async {
                let mut response = StatusCode::NO_CONTENT.into_response();
                response.headers_mut().insert(
                    CONTENT_TYPE,
                    HeaderValue::from_static("application/vnd.vapor.admin-page.v1+html"),
                );
                response
            }),
        );
        let response = owner
            .layer(axum::middleware::from_fn(
                super::shell_composition_only_for_tests,
            ))
            .oneshot(
                Request::get("/admin/build-cache")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(!response.headers().contains_key(CONTENT_TYPE));
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert!(body.is_empty());
    }

    #[tokio::test]
    async fn registry_admin_is_linked_and_unimplemented_modules_stay_planned() {
        let response = admin_app()
            .oneshot(
                Request::builder()
                    .uri("/admin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let html = std::str::from_utf8(&body).unwrap();

        assert!(html.contains("href=\"/admin/registry\""));
        assert!(html.contains("href=\"/admin/artifacts\""));
        assert!(!html.contains("Workshop</span><small class=\"nav-status\">Planned"));
        assert!(html.contains("href=\"/admin/build-cache\""));
    }

    #[test]
    fn aggregate_navigation_badges_name_all_required_services() {
        let approvals = super::render_admin_navigation("/admin/approvals");
        let hosting = super::render_admin_navigation("/admin/hosting");
        assert!(approvals.contains("data-admin-services=\"platform,identity\""));
        assert!(hosting.contains(
            "data-admin-services=\"platform,homepage,docs,identity,diagnostics,registry,artifacts,build-cache\""
        ));
    }

    #[test]
    fn shared_shell_exposes_the_server_owned_deployment_target_allowlist() {
        let header = super::render_admin_header("/admin/approvals");
        assert!(header.contains(
        "data-deployment-targets=\"atlas,homepage,docs,identity,diagnostics,registry,artifact,build-cache,platform\""
        ));
    }

    #[tokio::test]
    async fn monolith_probes_platform_and_homepage_as_distinct_services() {
        let app = Router::new().fallback(any(fake_monolith_health));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let services =
            super::probe_services_for_mode(&reqwest::Client::new(), &origin, false).await;
        let platform = services
            .iter()
            .find(|service| service.id == "platform")
            .unwrap();
        let homepage = services
            .iter()
            .find(|service| service.id == "homepage")
            .unwrap();
        assert_eq!(platform.health, "unavailable");
        assert_eq!(homepage.health, "healthy");

        server.abort();
    }

    #[tokio::test]
    async fn platform_health_route_is_public_and_non_sensitive() {
        let response = admin_app()
            .oneshot(
                Request::builder()
                    .uri("/api/admin/platform-healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(body.as_ref(), b"ok\n");
    }

    #[tokio::test]
    async fn rendered_shell_contains_no_operator_data() {
        let response = admin_app()
            .oneshot(
                Request::builder()
                    .uri("/admin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let html = std::str::from_utf8(&body).unwrap();

        assert!(!html.contains("VAPOR_IDENTITY_ADMIN_TOKEN"));
        assert!(!html.contains("/etc/vapor-server"));
        assert!(!html.contains("Set-Cookie"));
    }

    #[tokio::test]
    async fn operator_responses_are_private_no_store() {
        let response = admin_app()
            .oneshot(
                Request::builder()
                    .uri("/admin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.headers().get("cache-control").unwrap(),
            "private, no-store"
        );
    }

    #[tokio::test]
    async fn overview_rejects_missing_session_and_is_not_cacheable() {
        let response = admin_app()
            .oneshot(
                Request::builder()
                    .uri("/api/admin/overview")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response.headers().get("cache-control").unwrap(),
            "private, no-store"
        );
    }

    #[test]
    fn approvals_view_is_registered() {
        assert!(super::OperationsView::parse(Some("approvals")).is_some());
        assert!(matches!(
            super::OperationsView::parse(None),
            Some(super::OperationsView::Approvals)
        ));
        let html = super::operations_page(super::OperationsView::Approvals);
        assert!(html.contains("id=\"pending-deploy-approvals\""));
        assert!(html.contains("id=\"pending-ssh-approvals\""));
        assert!(html.contains("id=\"pending-identity-approvals\""));
        assert!(html.contains("id=\"approvals-more-deploy\""));
        assert!(html.contains("id=\"approvals-more-ssh\""));
        assert!(html.contains("id=\"approvals-more-identity\""));
        assert!(html.contains("Each decision is recorded by the service that owns it"));
        assert!(html.contains("<h1>Approvals</h1>"));
        let nav = super::render_admin_navigation("/admin/approvals");
        let approvals_link = nav
            .split("</a>")
            .find(|link| link.contains("href=\"/admin/approvals\""))
            .unwrap();
        assert!(approvals_link.contains("aria-current=\"page\""));
        assert!(nav.contains("href=\"/admin/deployments\""));
        assert!(nav.contains("href=\"/admin/ssh-access\""));
        let deployment_page = super::operations_page(super::OperationsView::Deployments);
        assert!(deployment_page.contains("<h1>Deployments</h1>"));
        assert!(!deployment_page.contains("id=\"pending-deploy-approvals\""));
    }

    #[tokio::test]
    async fn approvals_page_requires_root_and_has_owner_routed_actions() {
        for path in ["/admin/approvals", "/admin/operations?view=approvals"] {
            let response = admin_app()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            assert_eq!(
                response.headers().get("cache-control").unwrap(),
                "private, no-store"
            );
        }

        for path in [
            "/admin/deployments",
            "/admin/ssh-access",
            "/admin/hosting",
            "/admin/history",
        ] {
            let response = admin_app()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
        }

        let response = admin_app()
            .oneshot(
                Request::builder()
                    .uri("/admin/assets/admin.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let script = std::str::from_utf8(&body).unwrap();
        assert!(script.contains("'/api/admin/deploy-requests'"));
        assert!(script.contains("'/api/admin/ssh-access/requests'"));
        assert!(script.contains("/api/identity/v1/admin/permission-requests/page?limit=200"));
        assert!(script
            .contains("item.status === 'pending' && typeof item.current_main_commit === 'string'"));
        assert!(
            script.contains("/api/identity/v1/admin/permission-requests/${encodedId}/${action}")
        );
        assert!(script.contains(
            "This request source is temporarily unavailable or returned an invalid response."
        ));
        assert!(script.contains("records.filter(item => item && item.status === 'pending')"));
        assert!(script.contains("const requestMetadata = content =>"));
        assert!(script.contains("Request metadata"));
        assert!(script.contains("card.append(requestMetadata(metadata))"));
        assert!(script.contains("const isExpiredByTime = item => Number(item.expires_at_unix) > 0"));
        assert!(script.contains("pending.filter(isExpiredByTime)"));
        assert!(script.contains("reviewablePending.filter(item => !isExpiredByTime(item))"));
        assert!(script.contains("Expired at ${humanTime(item.expires_at_unix)} · Identity has not yet recorded the transition"));
        assert!(script.contains("renderExpiredIdentity(item, card)"));
        assert!(script.contains("reason: reason.value.trim()"));
        assert!(script.contains("granted_capabilities: granted"));
        assert!(script.contains("decision_saved === true"));
    }

    #[tokio::test]
    async fn operations_page_requires_root_and_has_no_mutation_routes() {
        let get_response = admin_app()
            .oneshot(
                Request::builder()
                    .uri("/admin/operations")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            get_response.headers().get("content-type").unwrap(),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            get_response.headers().get("cache-control").unwrap(),
            "private, no-store"
        );
        let body = get_response.into_body().collect().await.unwrap().to_bytes();
        let document = std::str::from_utf8(&body).unwrap();
        assert!(document.contains("Operator sign-in required"));
        assert!(document.contains("/operator/login"));
        assert!(document.contains("<nav aria-label=\"Platform administration\">"));

        let post_response = admin_app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/admin/operations")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(post_response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn operations_auth_errors_are_shell_fragments_not_api_json() {
        for (failure, expected_status, expected_text) in [
            (
                crate::admin_auth::AuthFailure::Unauthorized,
                StatusCode::UNAUTHORIZED,
                "Operator sign-in required",
            ),
            (
                crate::admin_auth::AuthFailure::Forbidden,
                StatusCode::FORBIDDEN,
                "Root access required",
            ),
            (
                crate::admin_auth::AuthFailure::Unavailable,
                StatusCode::SERVICE_UNAVAILABLE,
                "Identity authorization is temporarily unavailable",
            ),
        ] {
            let response = super::admin_page_auth_response(failure);
            assert_eq!(response.status(), expected_status);
            assert_eq!(
                response.headers()["content-type"],
                "application/vnd.vapor.admin-page.v1+html; charset=utf-8"
            );
            assert_eq!(response.headers()["cache-control"], "private, no-store");
            let body = response.into_body().collect().await.unwrap().to_bytes();
            let fragment = std::str::from_utf8(&body).unwrap();
            assert!(fragment.contains(expected_text));
            assert!(!fragment.starts_with('{'));
        }
    }

    #[tokio::test]
    async fn unauthenticated_admin_shell_points_to_operator_sign_in() {
        let response =
            super::admin_shell_auth_response(crate::admin_auth::AuthFailure::Unauthorized);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let html = std::str::from_utf8(&body).unwrap();

        assert!(html.contains("Operator sign-in required"));
        assert!(html.contains("/operator/login"));
        assert!(
            html.contains("normal Vapor account sign-in does not open the Platform admin session")
        );
        assert!(!html.contains("href=\"/login\""));
    }

    #[tokio::test]
    async fn admin_page_auth_challenge_explains_the_separate_operator_session() {
        let response =
            super::admin_page_auth_response(crate::admin_auth::AuthFailure::Unauthorized);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let html = std::str::from_utf8(&body).unwrap();

        assert!(
            html.contains("normal Vapor account sign-in does not open the Platform admin session")
        );
        assert!(html.contains("href=\"/operator/login\""));
        assert!(!html.contains("href=\"/login\""));
    }

    #[test]
    fn current_operation_pointer_lives_with_deployments_not_history() {
        let deployments = super::operations_page(super::OperationsView::Deployments);
        let history = super::operations_page(super::OperationsView::History);
        assert!(deployments.contains("Latest deploy or restore result"));
        assert!(deployments.contains("id=\"latest-operation-status\""));
        assert!(!history.contains("latest-operation-status"));
        assert!(history.contains("Platform · Records"));
        assert!(history.contains("Needs attention"));
        assert!(history.contains("In progress"));
        assert!(history.contains("Completed"));
        assert!(history.contains("class=\"history-source-panel\""));
        assert!(history.contains("id=\"operations-history\""));
        assert!(history.contains("id=\"history-load-more\""));
    }

    #[test]
    fn history_uses_owner_cursors_to_load_older_platform_records() {
        let script = super::JS;
        let page = super::operations_page(super::OperationsView::History);
        assert!(page.contains("Platform operation</option>"));
        assert!(script.contains("/api/admin/operations/history"));
        assert!(script.contains("/api/identity/v1/admin/permission-requests/page"));
        assert!(script.contains("const url = `${source.url}?limit=50`;"));
        assert!(script
            .contains("record.title = `${safeText(item.operation)} · ${safeText(item.result)}`"));
        assert!(script.contains("next_cursor"));
        assert!(script.contains("cursor: state.nextCursor"));
        assert!(script.contains("limit: '50'"));
        assert!(script.contains("loadMoreButton?.addEventListener('click', loadMoreHistory)"));
        assert!(script.contains("className = 'history-workbench'"));
        assert!(script.contains("className = 'history-record-list'"));
        assert!(script.contains("const historyStatus = record =>"));
        assert!(script.contains("record.kind === 'Identity request' && status === 'approved'"));
        assert!(script.contains("record.kind === 'Temporary access' && status === 'active'"));
        assert!(script.contains("const isOpenHistoryRecord = (source, item) =>"));
        assert!(
            script.contains("source.kind !== 'Platform operation' && item.status === 'pending'")
        );
        assert!(script.contains("if (isOpenHistoryRecord(source, item))"));
        assert!(script.contains("Pending requests are in"));
        assert!(script.contains("record.id === selectedRecordId"));
        assert!(!script.contains("className = 'history-entry'"));
    }

    #[tokio::test]
    async fn operations_error_page_loads_the_platform_owned_shared_shell() {
        let response = admin_app()
            .oneshot(
                Request::get("/admin/operations?view=unknown")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert!(std::str::from_utf8(&body)
            .unwrap()
            .contains("/admin/assets/admin.js"));
    }

    #[tokio::test]
    async fn ssh_access_ui_distinguishes_authorization_failures_and_shows_safe_api_details() {
        let response = admin_app()
            .oneshot(
                Request::builder()
                    .uri("/admin/assets/admin.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let script = std::str::from_utf8(&body).unwrap();
        assert!(script
            .contains("Sign in with an Identity root account to review temporary SSH requests."));
        assert!(script.contains("Your signed-in account does not have the Platform root role."));
        assert!(script.contains("const responseFailure = async (response, fallback) =>"));
        assert!(script.contains("awaiting review"));
        assert!(script.contains("const sshAccessList = document.querySelector('#ssh-access-list')"));
        assert!(script.contains("let nextCursor = null;"));
        assert!(script.contains("cursor: nextCursor"));
        assert!(script.contains("ssh-access-more"));
        assert!(script.contains("const updatedRecords = Array.isArray(body) ? body : body?.items;"));
        assert!(script.contains("if (!Array.isArray(updatedRecords))"));
        assert!(script.contains("const matchesFilter = item =>"));
        assert!(
            script.contains("No response received. Refresh to verify whether access was revoked.")
        );
        assert!(
            script.contains("/api/admin/ssh-access/requests/${encodeURIComponent(item.id)}/events")
        );
        assert!(script.contains("Show request history"));
        assert!(script.contains("if (item.granted_duration_seconds != null)"));
        assert!(script.contains("const capabilityLabel = value =>"));
        assert!(script.contains("const formatDuration = seconds =>"));
        assert!(script.contains("Grant duration (hours)"));
        assert!(script.contains("Number(duration.value) * 3600"));
        assert!(script.contains("item.status !== 'pending' && matchesFilter(item)"));
        assert!(script.contains("Temporary administrator access (unrestricted sudo)"));
        assert!(script.contains("not direct root SSH"));
    }

    #[test]
    fn ssh_access_page_is_a_list_and_detail_workbench() {
        let page = super::operations_page(super::OperationsView::TemporaryAccess);
        assert!(page.contains("id=\"ssh-access-search\""));
        assert!(page.contains("id=\"ssh-access-filter\""));
        assert!(page.contains("id=\"ssh-access-list\""));
        assert!(page.contains("id=\"ssh-access-detail\""));
        assert!(page.contains("Active grants"));
        assert!(!page.contains("value=\"pending\">Pending review"));
        assert!(page.contains("Pending requests are reviewed in"));
        assert!(page.contains("SSH access workbench"));
        assert!(page.contains("id=\"ssh-access-more\""));
        assert!(!page.contains("Loading access requests and active grants…"));
    }

    #[test]
    fn approvals_inbox_collapses_owner_queues_and_shows_live_counts() {
        let page = super::approvals_page_content();
        assert!(page.contains("Decision inbox"));
        assert!(page.contains("class=\"approval-source\" open"));
        assert!(page.contains("id=\"pending-deploy-count\""));
        assert!(page.contains("id=\"pending-ssh-count\""));
        assert!(page.contains("id=\"pending-identity-count\""));
        assert!(page.contains("id=\"approvals-more-identity\""));
        assert!(!page.contains("limited to the newest 200 requests"));
        let script = super::JS;
        assert!(script.contains("approval-deploy-header"));
        assert!(script.contains("Approve current main commit"));
        assert!(!script.contains("I reviewed this exact revision and target"));
    }

    #[test]
    fn server_fleet_is_a_distinct_private_page_and_navigation_destination() {
        let page = super::server_fleet_page_content();
        let nav = super::render_admin_navigation("/admin/servers");
        assert!(nav.contains("href=\"/admin/servers\""));
        assert!(nav.contains("Infrastructure"));
        assert!(page.contains("id=\"server-fleet-list\""));
        assert!(page.contains("id=\"server-fleet-refresh\""));
        assert!(page.contains("role=\"tablist\""));
        assert_eq!(page.matches("id=\"server-panel-services\"").count(), 1);
        assert!(page.contains("data-server-tab=\"overview\""));
        assert!(page.contains("data-server-tab=\"diagnostics\""));
        assert!(page.contains("id=\"server-network-range\""));
        assert!(page.contains("id=\"server-network-chart\""));
        assert!(page.contains("data-server-tab=\"services\""));
        assert!(page.contains("data-server-tab=\"access\""));
        assert!(page.contains("data-server-tab=\"console\""));
        assert!(page.contains("data-server-tab=\"connections\""));
        assert!(page.contains("Add server"));
        assert!(page.contains("Reconnect replacement"));
        assert!(!page.to_ascii_lowercase().contains("enroll"));
        assert!(page.contains("id=\"server-node-slot\""));
        assert!(page.contains("machine you already own"));
        assert!(page.contains("id=\"server-node-secret-download\""));
        assert!(page.contains("id=\"server-node-secret-value\" hidden"));
        assert!(page.contains("process-level resource readings are not collected per machine yet"));
        assert!(page.contains("/admin/ssh-access"));
        assert!(page.contains("/admin/emergency-console"));
        assert!(page.contains("/admin/hosting"));
        assert!(page.contains("id=\"server-module-actions\""));
        assert!(page.contains("data-service=\"homepage\""));
        assert!(page.contains("data-service=\"build_cache\""));
        assert!(page.contains("id=\"server-action-reboot\""));
        assert!(page.contains("id=\"server-operation-list\""));
        assert!(page.contains("limited to the core server"));
        assert!(!page.contains("<option value=\"platform\">Platform</option>"));
    }

    #[tokio::test]
    async fn server_action_ui_uses_typed_same_origin_operation_requests() {
        let response = admin_app()
            .oneshot(
                Request::builder()
                    .uri("/admin/assets/admin.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let script = std::str::from_utf8(&body).unwrap();
        assert!(script.contains("/api/admin/server-operations"));
        assert!(script.contains("kind: 'restart_service', service"));
        assert!(script.contains("crypto.randomUUID()"));
        assert!(script.contains("Check action history before retrying."));
        assert!(script.contains("window.confirm(`Restart ${label} on ${target}?"));
        assert!(script.contains("Type exactly:\\n${expected}"));
        assert!(script.contains("kind: 'reboot_host', confirmation"));
        assert!(script.contains("event.key === 'ArrowRight'"));
        assert!(script.contains("aria-selected"));
        assert!(script.contains("data-server-panel"));
        assert!(script.contains("/network?window="));
        assert!(script.contains("cache: 'no-store'"));
        assert!(script.contains("AbortSignal.timeout(6000)"));
        assert!(script.contains("chart green RX / blue TX"));
        assert!(script.contains("telemetry only · typed controls unavailable"));
    }

    #[test]
    fn control_workflows_are_top_level_navigation_tasks() {
        let nav = super::render_admin_navigation("/admin/ssh-access");
        assert!(nav.contains("<summary class=\"nav-group\">Operate</summary>"));
        assert!(nav.contains("<summary class=\"nav-group\">Ecosystem</summary>"));
        assert!(nav.contains("<summary class=\"nav-group\">Accounts</summary>"));
        assert!(nav.contains("href=\"/admin/approvals\""));
        assert!(nav.contains("href=\"/admin/deployments\""));
        assert!(nav.contains("href=\"/admin/ssh-access\""));
        assert!(!nav.contains("class=\"nav-group\">Operations</p>"));
    }

    #[tokio::test]
    async fn deploy_ui_separates_decision_and_dispatch_and_uses_retry_eligibility() {
        let response = admin_app()
            .oneshot(
                Request::builder()
                    .uri("/admin/assets/admin.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let script = std::str::from_utf8(&body).unwrap();
        assert!(script.contains(
            "${lifecycleLabel || dispatchLabels[item.dispatch_state] || 'Stage not reported'}"
        ));
        assert!(script.contains("expired: 'Expired before worker claim'"));
        assert!(script.contains("claimed: 'Worker claimed deployment'"));
        assert!(script.contains("completed: 'Worker finished deployment'"));
        assert!(script.contains("Worker stopped; outcome unknown"));
        assert!(script.contains("secondaryLabel: 'Execution follow-up'"));
        assert!(script.contains("Approval saved; deployment is queued for the worker."));
        assert!(
            script.contains("Approval saved; the worker claimed the deployment and it is running.")
        );
        assert!(script.contains("Approval saved; deployment succeeded."));
        assert!(script.contains("checkbox.addEventListener('invalid'"));
        assert!(script.contains("Confirm this action before submitting the request."));
        assert!(script.contains("const requireConfirmation = checkbox =>"));
        assert!(script.contains("requireConfirmation(confirmed)"));
        assert!(script.contains("requireConfirmation(confirm)"));
        assert!(script.contains("Select at least one capability for this temporary grant."));
        assert!(script.contains("let deploymentCursor = null;"));
        assert!(script.contains("cursor: deploymentCursor"));
        assert!(script.contains("deployment-refresh"));
        assert!(script.contains("deployments-more"));
        assert!(script
            .contains("dispatch_attempt_superseded: 'was superseded by a newer request state'"));
        assert!(script.contains(
            "const link = document.createElement('a'); link.href = '/admin/deployments';"
        ));
        assert!(script.contains("const validDeploymentSelection = item =>"));
        assert!(script.contains("const isQueuedOrRunning = item =>"));
        assert!(script.contains("const isDispatchUncertain = item =>"));
        assert!(script.contains("const isFailedDispatch = item =>"));
        assert!(script.contains("['Queued or running', queuedOrRunningItems]"));
        assert!(script.contains("['Dispatch outcome uncertain', uncertainItems]"));
        assert!(script.contains("['Dispatch failed before the worker started', failedItems]"));
        assert!(script.contains("No queued, running, uncertain, or failed-dispatch records."));
        assert!(script.contains("serviceDeployTargets.has(item.service_target)"));
        assert!(script.contains("document.querySelector('[data-deployment-targets]')"));
        assert!(script.contains("Target: ${deploymentSelectionLabel(item)}"));
        assert!(script.contains(
            "const canRetry = validDeploymentSelection(item) && item.status === 'approved'"
        ));
        assert!(script.contains(
            "&& item.retry_eligible && ['failed', 'unknown'].includes(item.dispatch_state)"
        ));
        assert!(script.contains("if (canRetry) actions.push('dispatch')"));
        assert!(script.contains("item.status === 'approved' && item.dispatch_state === 'failed') actions.push('retire')"));
        assert!(script.contains("Close failed dispatch"));
        assert!(script.contains("reason: reason.value.trim()"));
        assert!(script.contains("Closed before deployment"));
        assert!(script.contains("Dispatch failed before worker claim"));
        assert!(script.contains("body?.detail"));
        assert!(script.contains("const body = await result.clone().json()"));
        assert!(script.contains("actionFeedback.set(item.id, String(message).slice(0, 240))"));
    }

    #[tokio::test]
    async fn approvals_are_the_only_mutation_surface_for_pending_requests() {
        let response = admin_app()
            .oneshot(
                Request::builder()
                    .uri("/admin/assets/admin.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let script = std::str::from_utf8(&body).unwrap();
        assert!(script.contains("item.approval_order_ready === true"));
        assert!(script.contains("item.approval_order_blocker && typeof item.approval_order_blocker.id === 'string'"));
        assert!(script.contains("Blocked by ${orderBlocker.id.slice(0, 8)}"));
        assert!(script.contains("Waiting for prior deployment"));
        assert!(script.contains("created_at_unix || 0) - Number(b.created_at_unix || 0)"));
        let deployments = script
            .split("const deployRequests = document.querySelector('#deploy-requests');")
            .nth(1)
            .unwrap()
            .split("const sshAccessList =")
            .next()
            .unwrap();
        assert!(deployments.contains(".filter(item => item && (isQueuedOrRunning(item) || isDispatchUncertain(item) || isFailedDispatch(item)))"));
        assert!(deployments.contains("/admin/approvals#pending-deploy-approvals"));
        assert!(!deployments.contains("Approve deploy"));
        assert!(!deployments.contains("Reject"));
        assert!(deployments.contains("Retry deploy dispatch"));

        let ssh_access = script
            .split("const sshAccessList = document.querySelector('#ssh-access-list');")
            .nth(1)
            .unwrap()
            .split("const operationHistory =")
            .next()
            .unwrap();
        assert!(ssh_access.contains("item.status !== 'pending'"));
        assert!(!ssh_access.contains("/admin/approvals#pending-ssh-approvals"));
        assert!(!ssh_access.contains("/requests/${encodeURIComponent(item.id)}/approve"));
        assert!(!ssh_access.contains("/requests/${encodeURIComponent(item.id)}/reject"));
        assert!(ssh_access.contains("/grants/${encodeURIComponent(item.id)}/revoke"));

        let approvals = script
            .split("const pendingDeployApprovals =")
            .nth(1)
            .unwrap()
            .split("const operationHistory =")
            .next()
            .unwrap();
        assert!(approvals.contains("Approve current main commit"));
        assert!(approvals.contains("Approve selected access"));
        assert!(approvals.contains("approvals-more-deploy"));
        assert!(approvals.contains("approvals-more-ssh"));
        assert!(approvals.contains("approvals-more-identity"));
        assert!(approvals.contains("permission-requests/page?limit=200"));
        assert!(approvals.contains("pageButtons.identity?.addEventListener"));
        assert!(approvals.contains("pending · more available"));
        assert!(approvals.contains("next_cursor"));
        assert!(approvals.contains("Confirm the review before approving this SSH request."));
        assert!(approvals.contains("review.required = false"));
    }

    #[tokio::test]
    async fn aggregate_health_badges_refresh_on_every_shell_page() {
        let response = admin_app()
            .oneshot(
                Request::builder()
                    .uri("/admin/assets/admin.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let script = std::str::from_utf8(&body).unwrap();
        assert!(script.contains("[data-admin-services]"));
        assert!(script.contains("required.some(service => !service"));
        assert!(script.contains("healthyCount === 0 ? 'unavailable' : 'degraded'"));
        assert!(script.contains("if (!overview) return;"));
    }

    #[tokio::test]
    async fn operations_history_api_requires_root_before_reading_record() {
        let response = admin_app()
            .oneshot(
                Request::builder()
                    .uri("/api/admin/operations")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response.headers().get("cache-control").unwrap(),
            "private, no-store"
        );
    }

    #[tokio::test]
    async fn operation_record_accepts_only_the_bounded_current_schema() {
        let directory = test_directory("operation-record-valid");
        tokio::fs::create_dir_all(&directory).await.unwrap();
        let path = directory.join("latest.json");
        tokio::fs::write(
            &path,
            r#"{"schema_version":1,"operation":"deploy","result":"success","started_at_utc":"2026-11-02T12:00:00Z","finished_at_utc":"2026-11-02T12:01:00Z","revision":"0123456789abcdef0123456789abcdef01234567","server_mode":"monolith"}"#,
        )
        .await
        .unwrap();

        let record = super::read_operation_record(&path).await.unwrap().unwrap();
        assert_eq!(record.operation, "deploy");
        assert_eq!(record.result, "success");
        assert_eq!(record.server_mode, "monolith");

        tokio::fs::remove_dir_all(directory).await.unwrap();
    }

    #[tokio::test]
    async fn operation_record_rejects_unknown_fields_bad_timestamps_and_oversize_files() {
        let directory = test_directory("operation-record-invalid");
        tokio::fs::create_dir_all(&directory).await.unwrap();
        let path = directory.join("latest.json");
        let valid = r#"{"schema_version":1,"operation":"restore","result":"failed","started_at_utc":"2026-11-02T12:00:00Z","finished_at_utc":"2026-11-02T12:01:00Z","revision":"unknown","server_mode":"services"}"#;

        tokio::fs::write(
            &path,
            valid.replace(
                "\"result\":\"failed\"",
                "\"result\":\"failed\",\"private_path\":\"/etc/secret\"",
            ),
        )
        .await
        .unwrap();
        assert!(super::read_operation_record(&path).await.is_err());

        tokio::fs::write(&path, valid.replace("12:00:00", "xx:00:00"))
            .await
            .unwrap();
        assert!(super::read_operation_record(&path).await.is_err());

        tokio::fs::write(&path, vec![b'x'; 4097]).await.unwrap();
        assert!(super::read_operation_record(&path).await.is_err());

        tokio::fs::remove_dir_all(directory).await.unwrap();
    }

    #[tokio::test]
    async fn missing_operation_record_is_not_reported_not_an_error() {
        let directory = test_directory("operation-record-missing");
        tokio::fs::create_dir_all(&directory).await.unwrap();
        let path = directory.join("missing.json");

        assert!(super::read_operation_record(&path).await.unwrap().is_none());

        tokio::fs::remove_dir_all(directory).await.unwrap();
    }

    #[tokio::test]
    async fn operations_status_rejects_private_fields_without_echoing_them() {
        let directory = test_directory("operation-record-redaction");
        tokio::fs::create_dir_all(&directory).await.unwrap();
        let path = directory.join("latest.json");
        tokio::fs::write(
            &path,
            r#"{"schema_version":1,"operation":"deploy","result":"failed","started_at_utc":"2026-11-02T12:00:00Z","finished_at_utc":"2026-11-02T12:01:00Z","revision":"unknown","server_mode":"monolith","private_path":"/srv/private/deploy.log","token":"must-not-escape"}"#,
        )
        .await
        .unwrap();

        let state = OverviewMockState {
            authorization_requests: Arc::new(AtomicUsize::new(0)),
            health_requests: Arc::new(AtomicUsize::new(0)),
        };
        let authorization_requests = state.authorization_requests.clone();
        let app = Router::new()
            .route("/authorize", post(mock_authorize))
            .with_state(state);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/authorize", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            "cookie",
            HeaderValue::from_str(&format!("vapor_identity_session={SESSION}")).unwrap(),
        );

        let response = super::operations_status_with_client(
            &headers,
            Some(SERVICE_TOKEN),
            &endpoint,
            &client,
            &path,
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(authorization_requests.load(Ordering::SeqCst), 1);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert!(body
            .windows(b"invalid".len())
            .any(|window| window == b"invalid"));
        assert!(!body
            .windows(b"/srv/private/deploy.log".len())
            .any(|window| window == b"/srv/private/deploy.log"));
        assert!(!body
            .windows(b"must-not-escape".len())
            .any(|window| window == b"must-not-escape"));

        server.abort();
        tokio::fs::remove_dir_all(directory).await.unwrap();
    }

    fn test_directory(label: &str) -> std::path::PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        std::env::temp_dir().join(format!(
            "vapor-platform-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[tokio::test]
    async fn overview_authorizes_root_then_returns_only_health_summaries() {
        let state = OverviewMockState {
            authorization_requests: Arc::new(AtomicUsize::new(0)),
            health_requests: Arc::new(AtomicUsize::new(0)),
        };
        let authorization_requests = state.authorization_requests.clone();
        let health_requests = state.health_requests.clone();
        let app = Router::new()
            .route("/authorize", post(mock_authorize))
            .fallback(any(mock_probe))
            .with_state(state);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let endpoint = format!("{origin}/authorize");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            "cookie",
            HeaderValue::from_str(&format!("vapor_identity_session={SESSION}")).unwrap(),
        );

        let response = overview_with_clients(
            &headers,
            Some(SERVICE_TOKEN),
            &endpoint,
            &origin,
            &client,
            &client,
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("cache-control").unwrap(),
            "private, no-store"
        );
        assert_eq!(authorization_requests.load(Ordering::SeqCst), 1);
        assert_eq!(health_requests.load(Ordering::SeqCst), 9);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let overview: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(overview["state"], "serving");
        let service_health = |id: &str| {
            overview["services"]
                .as_array()
                .unwrap()
                .iter()
                .find(|service| service["id"] == id)
                .unwrap()["health"]
                .as_str()
                .unwrap()
        };
        assert_eq!(service_health("platform"), "healthy");
        assert_eq!(service_health("homepage"), "healthy");
        assert_eq!(service_health("docs"), "unavailable");
        assert_eq!(service_health("build-cache"), "healthy");
        let build_cache = overview["services"]
            .as_array()
            .unwrap()
            .iter()
            .find(|service| service["id"] == "build-cache")
            .unwrap();
        assert_eq!(
            build_cache["readiness"],
            "0/2 snapshots current; 0/2 targets configured"
        );
        assert_eq!(overview["services"][0]["readiness"], "not reported");
        assert!(!body
            .windows(b"private-probe-payload".len())
            .any(|window| window == b"private-probe-payload"));
        server.abort();
    }

    #[test]
    fn status_text_is_html_escaped() {
        assert_eq!(
            super::escape_html("<script a='x'>&\""),
            "&lt;script a=&#39;x&#39;&gt;&amp;&quot;"
        );
    }

    #[tokio::test]
    async fn overview_health_is_per_service_and_discards_probe_bodies() {
        let app = Router::new().fallback(any(fake_health));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = reqwest::Client::new();
        let services = super::probe_services(&client, &origin).await;
        let docs = services
            .iter()
            .find(|service| service.id == "docs")
            .unwrap();
        let platform = services
            .iter()
            .find(|service| service.id == "platform")
            .unwrap();
        let build_cache = services
            .iter()
            .find(|service| service.id == "build-cache")
            .unwrap();

        assert_eq!(docs.health, "unavailable");
        assert_eq!(platform.health, "healthy");
        assert_eq!(build_cache.health, "healthy");
        assert_eq!(
            build_cache.readiness,
            "0/2 snapshots current; 0/2 targets configured"
        );
        let serialized = serde_json::to_string(&services).unwrap();
        assert!(!serialized.contains("private-probe-payload"));
        server.abort();
    }

    #[test]
    fn build_cache_readiness_distinguishes_service_health_from_target_state() {
        let status = json!({
            "targets": [
                {"target_triple":"x86_64-unknown-linux-gnu","configured":false,"current":false},
                {"target_triple":"x86_64-pc-windows-gnullvm","configured":false,"current":false}
            ]
        });
        assert_eq!(
            super::build_cache_readiness(&status),
            "0/2 snapshots current; 0/2 targets configured"
        );
    }

    #[test]
    fn build_cache_readiness_counts_only_current_configured_supported_targets() {
        let status = json!({
            "targets": [
                {"target_triple":"x86_64-unknown-linux-gnu","configured":true,"current":true},
                {"target_triple":"x86_64-pc-windows-gnullvm","configured":true,"current":false},
                {"target_triple":"aarch64-unknown-linux-gnu","configured":true,"current":true}
            ]
        });
        assert_eq!(
            super::build_cache_readiness(&status),
            "1/2 snapshots current; 2/2 targets configured"
        );
    }

    #[test]
    fn build_cache_readiness_marks_incomplete_or_duplicate_target_reports_unknown() {
        let missing = json!({
            "targets": [
                {"target_triple":"x86_64-unknown-linux-gnu","configured":false,"current":false}
            ]
        });
        let duplicate = json!({
            "targets": [
                {"target_triple":"x86_64-unknown-linux-gnu","configured":false,"current":false},
                {"target_triple":"x86_64-unknown-linux-gnu","configured":false,"current":false},
                {"target_triple":"x86_64-pc-windows-gnullvm","configured":false,"current":false}
            ]
        });
        assert_eq!(super::build_cache_readiness(&missing), "unknown");
        assert_eq!(super::build_cache_readiness(&duplicate), "unknown");
    }

    #[tokio::test]
    async fn one_slow_health_probe_times_out_without_panicking() {
        let app = Router::new().route("/slow", any(slow_health));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(20))
            .build()
            .unwrap();

        assert_eq!(
            super::probe_health(&client, &origin, "/slow").await,
            "unavailable"
        );
        server.abort();
    }
}
