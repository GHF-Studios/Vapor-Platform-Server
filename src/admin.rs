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
        name: "Workshop / Registry",
        path: Some("/admin/workshop"),
        description: "Registry-owned catalog inventory",
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
        name: "Build Cache",
        group: "Ecosystem",
        path: Some("/admin/build-cache"),
        description: "Per-target compiler snapshots",
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
        group: "Operate",
        name: "History",
        path: Some("/admin/history"),
        description: "Recent owner-sourced request and operation history",
        service_ids: &["platform", "identity"],
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
.approval-source { margin:10px 0; background:#182330; border:1px solid #2b3a4b; border-radius:7px; overflow:hidden; }
.approval-source > summary { display:flex; justify-content:space-between; align-items:center; gap:12px; padding:12px 14px; cursor:pointer; list-style:none; font-weight:650; }
.approval-source > summary::-webkit-details-marker { display:none; }
.approval-source > summary::after { content:'+'; order:2; margin-left:auto; color:#8092a5; }
.approval-source[open] > summary::after { content:'−'; }
.approval-source-count { order:1; margin-left:auto; color:#9eacba; font-size:12px; font-weight:500; }
.approval-source-body { padding:0 14px 12px; }
.table-scroll { max-width:100%; overflow-x:auto; }
.table-scroll:focus-visible { outline:2px solid #8fc5f4; outline-offset:2px; }
.skip-link { position:absolute; left:-9999px; top:8px; z-index:5; padding:8px 12px; background:#fff; color:#111; }
.skip-link:focus { left:8px; }
a:focus-visible, button:focus-visible, input:focus-visible, select:focus-visible { outline:2px solid #8fc5f4; outline-offset:2px; }
.eyebrow { color:#80a8cd; text-transform:uppercase; letter-spacing:.12em; font-size:11px; }
h1 { margin:.35rem 0 1.25rem; font-size:26px; }
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
        response.status === 403 ? 'Root access required' : 'Unavailable';
      const state = response.status === 401 || response.status === 403 ? 'protected' : 'unavailable';
      setPill(operator, label, state);
      setPill(platform, response.status === 401 || response.status === 403 ? 'Protected' : 'Unavailable', state);
      updateNavHealth(null);
      return;
    }

    setPill(operator, 'Root session verified', 'healthy');
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
          message.textContent = response.status === 401 ? 'Sign in to view service status.' :
            response.status === 403 ? 'Root access is required to view service status.' :
            'Service status is temporarily unavailable.';
          overview.append(message);
          if (response.status === 401) {
            const login = document.createElement('a');
            login.href = '/login';
            login.textContent = 'Sign in';
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
        state.textContent = !services.length ? 'No service status reported' : unavailable.length ? `${unavailable.length} service${unavailable.length === 1 ? '' : 's'} need attention` : 'All services healthy';
        summaryText.append(state, document.createTextNode(` · ${services.length} services · revision ${body.deployed_revision || 'not recorded'}`));
        summary.append(summaryText);
        if (unavailable.length) {
          const hosting = document.createElement('a');
          hosting.href = '/admin/hosting';
          hosting.textContent = 'Open service health';
          summary.append(hosting);
        }
        overview.append(summary);

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
      { name: 'Deployments', url: '/api/admin/deploy-requests?limit=200', href: '/admin/approvals', summarize: items => ({ primary: items.filter(item => item.status === 'pending').length, label: 'Awaiting review', secondary: items.filter(item => item.status === 'approved' && ['failed', 'unknown', 'dispatching'].includes(item.dispatch_state)).length, secondaryLabel: 'Dispatch needs attention' }) },
      { name: 'Temporary access', url: '/api/admin/ssh-access/requests?limit=200', href: '/admin/approvals', summarize: items => ({ primary: items.filter(item => item.status === 'pending').length, label: 'Awaiting review', secondary: items.filter(item => item.status === 'active').length, secondaryLabel: 'Active grants' }) },
      { name: 'Identity permissions', url: '/api/identity/v1/admin/permission-requests', href: '/admin/approvals', summarize: items => { const pending = items.filter(item => item.status === 'pending'); const overdue = pending.filter(item => Number(item.expires_at_unix) > 0 && Number(item.expires_at_unix) <= Math.floor(Date.now() / 1000)).length; return { primary: pending.length - overdue, label: 'Awaiting review', secondary: overdue, secondaryLabel: 'Expired by time; owner transition pending' }; } }
    ];
    const renderQueue = async () => {
      if (refreshQueueButton?.disabled) return;
      if (refreshQueueButton) { refreshQueueButton.disabled = true; refreshQueueButton.textContent = 'Refreshing…'; }
      requestQueue.replaceChildren();
      const table = document.createElement('table'); table.className = 'request-queue-table';
      table.innerHTML = '<thead><tr><th scope="col">Workflow</th><th scope="col">Waiting</th><th scope="col">Additional</th><th scope="col">Open</th></tr></thead>';
      const body = document.createElement('tbody');
      const outcomes = await Promise.all(sources.map(async source => {
        try {
          const response = await fetch(source.url, { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(6000) });
          const items = await response.json().catch(() => null);
          if (!response.ok) return { source, error: response.status === 401 ? 'Sign in required' : response.status === 403 ? 'Root access required' : `Unavailable (HTTP ${response.status})` };
          const records = Array.isArray(items) ? items : items?.items;
          if (!Array.isArray(records)) return { source, error: 'Invalid service response' };
          return { source, counts: source.summarize(records) };
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
          primary.textContent = String(outcome.counts.primary);
          secondary.textContent = outcome.counts.secondary > 0 ? `${outcome.counts.secondary} · ${outcome.counts.secondaryLabel}` : '—';
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
      ssh: document.querySelector('#approvals-more-ssh')
    };
    const countBadges = {
      deploy: document.querySelector('#pending-deploy-count'),
      ssh: document.querySelector('#pending-ssh-count'),
      identity: document.querySelector('#pending-identity-count')
    };
    const sourceDefinitions = [
      { key: 'deploy', url: '/api/admin/deploy-requests?limit=200', label: 'Platform deployment approvals' },
      { key: 'ssh', url: '/api/admin/ssh-access/requests?limit=200', label: 'Platform SSH access requests' },
      { key: 'identity', url: '/api/identity/v1/admin/permission-requests', label: 'Identity permission requests' }
    ];
    const sourceState = Object.fromEntries(sourceDefinitions.map(source => [source.key, { records: [], cursor: null }]));
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
    const confirmationForm = (item, labelText, buttonText, submit) => {
      const form = document.createElement('form');
      form.className = 'inline';
      const label = document.createElement('label');
      const confirm = document.createElement('input');
      confirm.type = 'checkbox';
      confirm.required = true;
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
      appendText(card, 'h3', `${item.repository || 'Unknown repository'} · ${item.branch || 'unknown branch'}`);
      const selectionValid = validDeploymentSelection(item);
      appendText(card, 'p', `Commit ${item.commit || 'unknown'} · ${deploymentSelectionLabel(item)}`, selectionValid ? 'muted' : 'error');
      appendText(card, 'p', item.summary || 'No request summary provided.');
      const metadata = document.createElement('p'); metadata.className = 'muted';
      metadata.textContent = `Submitted by ${item.requester_kind || 'unknown'} · profile ${item.requester_profile_id || 'unknown'}`;
      card.append(requestMetadata(metadata));
      appendText(card, 'p', `Expires ${humanTime(item.expires_at_unix)}`, 'muted');
      try {
        const ciUrl = new URL(item.workflow_url);
        if (ciUrl.protocol === 'https:' && ciUrl.hostname === 'github.com' && ciUrl.pathname.startsWith('/GHF-Studios/')) {
          const link = document.createElement('a'); link.href = ciUrl.href; link.rel = 'noopener noreferrer';
          link.textContent = 'Open successful CI run'; card.append(link);
        }
      } catch (_) {}
      const pins = document.createElement('details');
      const summary = document.createElement('summary'); summary.textContent = `${Array.isArray(item.submodules) ? item.submodules.length : 0} pinned service revisions`;
      const list = document.createElement('ul');
      for (const pin of (Array.isArray(item.submodules) ? item.submodules : [])) appendText(list, 'li', `${pin.path}: ${pin.commit}`);
      pins.append(summary, list); card.append(pins);
      appendText(card, 'p', 'Approving records the decision and asks Platform to queue this exact target.', 'muted');
      card.append(confirmationForm(item, 'I reviewed this exact revision and target', 'Approve and queue', button => submitDecision(button, 'deploy', item, 'approve', { expected_version: item.version, confirmed: true, reason: 'approved in Platform Approvals' })));
      card.append(confirmationForm(item, 'I intend to reject this request', 'Reject', button => submitDecision(button, 'deploy', item, 'reject', { expected_version: item.version, confirmed: true, reason: 'rejected in Platform Approvals' })));
      if (!selectionValid) card.querySelector('form button').disabled = true;
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
      const label = document.createElement('label'); const confirm = document.createElement('input'); confirm.type = 'checkbox'; confirm.required = true;
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
      if (countBadges[source.key]) countBadges[source.key].textContent = `${pending.length} pending`;
      const expiredIdentity = source.key === 'identity' ? pending.filter(isExpiredByTime) : [];
      const actionable = source.key === 'identity'
        ? pending.filter(item => !isExpiredByTime(item))
        : pending;
      container.replaceChildren();
      if (!pending.length) appendText(container, 'p', 'No pending requests in the loaded records.', 'muted');
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
      if (pageButtons[source.key]) {
        pageButtons[source.key].hidden = !state.cursor;
        pageButtons[source.key].disabled = false;
        pageButtons[source.key].textContent = 'Load older requests';
      }
    };
    const readSourcePage = async (source, cursor) => {
      const state = sourceState[source.key];
      const url = new URL(source.url, window.location.href);
      if (cursor) {
        url.searchParams.set('limit', '200');
        url.searchParams.set('cursor', cursor);
      }
      const response = await fetch(url, { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(6000) });
      const body = await response.json().catch(() => null);
      if (!response.ok) {
        state.error = response.status === 401 ? 'Sign in required.' : response.status === 403 ? 'Root access required.' : `Unavailable (HTTP ${response.status}).`;
        throw new Error(state.error);
      }
      const items = Array.isArray(body) ? body : body?.items;
      if (!Array.isArray(items)) throw new Error('invalid response');
      state.records.push(...items);
      state.cursor = Array.isArray(body) ? null : (typeof body.next_cursor === 'string' ? body.next_cursor : null);
      state.error = null;
    };
    const refreshSource = async source => {
      const container = containers[source.key];
      const state = sourceState[source.key];
      state.records = [];
      state.cursor = null;
      container.textContent = 'Loading…';
      try {
        await readSourcePage(source, null);
        renderSource(source);
      } catch (_) {
        container.replaceChildren();
        if (countBadges[source.key]) countBadges[source.key].textContent = 'Unavailable';
        appendText(container, 'p', 'This request source is temporarily unavailable or returned an invalid response.', 'error');
      }
    };
    const loadMoreSource = async source => {
      const button = pageButtons[source.key];
      const state = sourceState[source.key];
      if (!button || button.disabled || !state.cursor) return;
      button.disabled = true;
      button.textContent = 'Loading…';
      try {
        await readSourcePage(source, state.cursor);
        renderSource(source);
      } catch (_) {
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
      { kind: 'Identity request', label: 'Identity permission requests', url: '/api/identity/v1/admin/permission-requests' }
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
          const url = source.kind === 'Identity request' ? source.url : `${source.url}?limit=50`;
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
    const isCurrentDeployment = item => ['approved', 'executing'].includes(item.status);
    const actionFeedback = new Map();
    const refreshDeployRequests = async () => {
      let response;
      let requests;
      try {
        response = await fetch('/api/admin/deploy-requests?limit=200', { credentials: 'same-origin', cache: 'no-store' });
        requests = await response.json().catch(() => null);
      } catch (_) {
        deployRequests.textContent = 'Deploy approvals are temporarily unavailable.';
        return;
      }
      deployRequests.replaceChildren();
      if (!response.ok) {
        deployRequests.textContent = response.status === 401 ? 'Sign in to review deploy requests.' :
          response.status === 403 ? 'Root access is required to review deploy requests.' :
          'Deploy approvals are temporarily unavailable.';
        return;
      }
      const items = (Array.isArray(requests) ? requests : (Array.isArray(requests?.items) ? requests.items : []))
        .filter(item => item && isCurrentDeployment(item));
      if (!items.length) {
        const approvalsLink = document.createElement('a'); approvalsLink.href = '/admin/approvals#pending-deploy-approvals';
        approvalsLink.textContent = 'Approvals';
        const historyLink = document.createElement('a'); historyLink.href = '/admin/history';
        historyLink.textContent = 'History';
        deployRequests.replaceChildren(
          document.createTextNode('No active deployments. Pending approvals are in '),
          approvalsLink,
          document.createTextNode('; completed and closed runs are in '),
          historyLink,
          document.createTextNode('.')
        );
        return;
      }
      for (const item of items) {
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
          item.status === 'failed' && item.dispatch_state === 'failed' ? 'Dispatch failed before worker claim' :
          item.status === 'failed' ? 'Deployment failed' :
          item.status === 'expired' ? 'Expired before worker claim' : null;
        const stage = document.createElement('p');
        stage.className = 'muted';
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
          confirmed.required = true;
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
    };
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
    let records = [];
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
      count.textContent = `${visible.length} grants shown · ${active} active · ${pending} awaiting review`;
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
      if (refreshButton) refreshButton.disabled = true;
      let response;
      try { response = await fetch('/api/admin/ssh-access/requests?limit=200', { credentials: 'same-origin', cache: 'no-store' }); }
      catch (_) {
        sshAccessList.innerHTML = '<li class="error">Could not contact the temporary SSH service.</li>';
        if (refreshButton) refreshButton.disabled = false; return;
      }
      if (!response.ok) { await showResponseFailure(sshAccessList, response, 'Could not load SSH access records'); if (refreshButton) refreshButton.disabled = false; return; }
      const body = await response.json().catch(() => null);
      records = Array.isArray(body) ? body : body?.items;
      if (!Array.isArray(records)) { sshAccessList.innerHTML = '<li class="error">The SSH service returned an invalid response.</li>'; if (refreshButton) refreshButton.disabled = false; return; }
      records.sort((a, b) => (b.created_at_unix || 0) - (a.created_at_unix || 0));
      renderList(); if (refreshButton) refreshButton.disabled = false;
    };
    search.addEventListener('input', renderList); filter.addEventListener('change', renderList);
    refreshButton?.addEventListener('click', refresh); refresh();
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
        .route("/admin/history", get(history))
        .route("/api/admin/overview", get(overview))
        .route("/api/admin/platform-healthz", get(platform_healthz))
        .route("/api/admin/operations", get(operations_status))
        .merge(crate::operation_history::router())
        .route("/admin/assets/admin.css", get(stylesheet))
        .route("/admin/assets/admin.js", get(script))
        .merge(crate::deploy_requests::router())
}

async fn platform_healthz() -> &'static str {
    "ok\n"
}

/// Apply the Platform-owned document frame to a versioned service page fragment.
/// Owner services keep authorization and page contents; this middleware only
/// composes the bounded fragment with Platform navigation and assets.
pub async fn shell_middleware(request: Request, next: Next) -> Response {
    let path = request.uri().path().to_owned();
    let is_admin_document = request.method() == axum::http::Method::GET
        && (path == "/admin" || path.starts_with("/admin/"))
        && !path.starts_with("/admin/assets/");
    let mut response = next.run(request).await;
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

fn admin_shell_error(status: StatusCode, current_path: &str) -> Response {
    let fragment = "<p class=\"eyebrow\">Platform</p><h1>Admin page unavailable</h1><p class=\"error\">The service returned an invalid or oversized page fragment.</p>";
    let body = render_admin_fragment_document(fragment, current_path).unwrap_or_else(|| {
        "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><title>Admin page unavailable</title><body><main><h1>Admin page unavailable</h1><p>The service returned an invalid or oversized page fragment.</p></main></body></html>".to_owned()
    });
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
        "<header class=\"admin-header\" data-deployment-targets=\"{}\"><div><p class=\"eyebrow\">Current section</p><p class=\"admin-section-name\">{}</p></div><div class=\"admin-statuses\"><p class=\"admin-status-item\">Operator session <span class=\"status-pill status-protected\" data-operator-session role=\"status\" aria-live=\"polite\">Checking</span></p><p class=\"admin-status-item\">Platform <span class=\"status-pill\" data-platform-status role=\"status\" aria-live=\"polite\">Checking</span></p></div></header>",
        escape_html(&deployment_targets),
        escape_html(section_name),
    )
}

fn render_admin_navigation(current_path: &str) -> String {
    let mut html = String::from(
        "<nav aria-label=\"Platform administration\"><p class=\"brand\">Vapor Platform</p>",
    );
    let group_order = ["Operate", "Ecosystem", "Accounts"];
    for group in group_order {
        let sections: Vec<_> = MODULES.iter().filter(|section| section.group == group).collect();
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
    let fragment = "<p class=\"eyebrow\">Platform</p><h1>Overview</h1><section class=\"card\" aria-labelledby=\"service-overview-title\"><div class=\"request-queue-header\"><h2 id=\"service-overview-title\">Services</h2></div><div id=\"platform-overview\" aria-live=\"polite\"><p class=\"muted\">Loading service health…</p></div></section><section class=\"card\" aria-labelledby=\"operator-queue-title\"><div class=\"request-queue-header\"><h2 id=\"operator-queue-title\">Incoming work</h2><button id=\"operator-queue-refresh\" type=\"button\">Refresh</button></div><p class=\"muted\">Waiting requests are shown by owner. Review them in Approvals; active access opens its own work area.</p><div id=\"operator-queue\" aria-live=\"polite\"><p class=\"muted\">Loading request queues…</p></div></section>";
    admin_page_response(StatusCode::OK, fragment)
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
        Err(_) => {
            return admin_page_auth_response(crate::admin_auth::AuthFailure::Unavailable)
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
        return admin_page_auth_response(failure);
    }
    admin_page_response(StatusCode::OK, operations_page(view))
}

fn approvals_page_content() -> &'static str {
    concat!(
        "<section class=\"card\"><div class=\"request-queue-header\"><div><h2>Decision inbox</h2><p class=\"muted\">Only unanswered requests appear here. Each decision is recorded by the service that owns it; deployment execution is tracked separately.</p></div><button id=\"approvals-refresh\" type=\"button\">Refresh</button></div><p id=\"approvals-feedback\" role=\"status\" aria-live=\"polite\"></p></section>",
        "<details class=\"approval-source\" open><summary>Deployments <span id=\"pending-deploy-count\" class=\"approval-source-count\">Loading…</span></summary><div class=\"approval-source-body\"><div id=\"pending-deploy-approvals\" aria-live=\"polite\"><p class=\"muted\">Loading deployment approvals…</p></div><button id=\"approvals-more-deploy\" type=\"button\" hidden>Load older requests</button></div></details>",
        "<details class=\"approval-source\"><summary>SSH access <span id=\"pending-ssh-count\" class=\"approval-source-count\">Loading…</span></summary><div class=\"approval-source-body\"><div id=\"pending-ssh-approvals\" aria-live=\"polite\"><p class=\"muted\">Loading SSH access requests…</p></div><button id=\"approvals-more-ssh\" type=\"button\" hidden>Load older requests</button></div></details>",
        "<details class=\"approval-source\"><summary>Identity permissions <span id=\"pending-identity-count\" class=\"approval-source-count\">Loading…</span></summary><div class=\"approval-source-body\"><p class=\"muted\">Identity list is currently limited to the newest 200 requests.</p><div id=\"pending-identity-approvals\" aria-live=\"polite\"><p class=\"muted\">Loading Identity permission requests…</p></div></div></details>"
    )
}

fn operations_page(view: OperationsView) -> String {
    let content = match view {
        OperationsView::Approvals => approvals_page_content().to_owned(),
        OperationsView::Deployments => concat!(
            "<section class=\"card\"><h2>Execution queue</h2><p class=\"muted\">This is the lifecycle after approval: pinned revisions, dispatch, worker progress, outcome, and eligible recovery actions. New decisions are made in <a href=\"/admin/approvals\">Approvals</a>.</p><div id=\"deploy-requests\" aria-live=\"polite\"><p class=\"muted\">Loading deployment execution…</p></div></section>",
            "<section class=\"card\"><h2>Latest deploy or restore result</h2><p class=\"muted\">This is the current Platform status pointer. Historical requests are listed separately under History.</p><div id=\"latest-operation-status\" aria-live=\"polite\"><p class=\"muted\">Loading latest operation result…</p></div><a href=\"/docs\">Open server runbooks</a></section>"
        )
        .to_owned(),
        OperationsView::Hosting => "<section class=\"card\"><h2>Service health and feature readiness</h2><p class=\"muted\">Health reports whether a service answers its probe. Feature readiness is shown only when the service reports it; “not reported” is not treated as ready.</p><div id=\"operations-status\" aria-live=\"polite\"><p class=\"muted\">Loading service status…</p></div></section><section class=\"card\"><h2>Runbooks</h2><p class=\"muted\">Use the service-owned recovery guidance for operational changes.</p><a href=\"/docs\">Open server runbooks</a></section>".to_owned(),
        OperationsView::TemporaryAccess => "<p class=\"eyebrow\">Operate · Access</p><div class=\"request-queue-header\"><div><h1>SSH access</h1><p class=\"muted\">Manage active grants and closed access records. Pending requests are reviewed in <a href=\"/admin/approvals#pending-ssh-approvals\">Approvals</a>.</p></div><button id=\"ssh-access-refresh\" type=\"button\">Refresh</button></div><section class=\"ssh-workbench\" aria-label=\"SSH access workbench\"><div class=\"ssh-list-panel\"><div class=\"ssh-list-toolbar\"><label for=\"ssh-access-search\">Find a grant<input id=\"ssh-access-search\" type=\"search\" placeholder=\"Purpose, fingerprint, account\" autocomplete=\"off\"></label><label for=\"ssh-access-filter\">Show<select id=\"ssh-access-filter\"><option value=\"active\">Active grants</option><option value=\"all\">All grants</option><option value=\"history\">Closed grants</option></select></label><p id=\"ssh-access-count\" class=\"ssh-list-count\" aria-live=\"polite\">Loading grants…</p></div><ul id=\"ssh-access-list\" class=\"ssh-record-list\" aria-label=\"SSH grants\"><li class=\"muted\">Loading…</li></ul></div><section id=\"ssh-access-detail\" class=\"ssh-detail-panel\" aria-live=\"polite\"><p class=\"muted\">Choose a grant to inspect its details.</p></section></section>".to_owned(),
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
    readiness: &'static str,
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
            Self::Approvals | Self::Deployments | Self::Hosting | Self::TemporaryAccess => "Operate",
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
            "<p class=\"eyebrow\">Platform · Operations</p><h1>Sign-in required</h1><p>Sign in with a root administrator account to view Operations.</p><a href=\"/login\">Sign in</a>"
        }
        crate::admin_auth::AuthFailure::Forbidden => {
            "<p class=\"eyebrow\">Platform · Operations</p><h1>Root access required</h1><p>This work area is available to root administrators.</p>"
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
        let (platform, homepage, docs, identity, diagnostics, registry, artifacts, build_cache) = tokio::join!(
            probe_health(client, "http://127.0.0.1:7110", "/healthz"),
            probe_health(client, "http://127.0.0.1:7111", "/healthz"),
            probe_health(client, "http://127.0.0.1:7112", "/healthz"),
            probe_health(client, "http://127.0.0.1:7113", "/healthz"),
            probe_health(client, "http://127.0.0.1:7114", "/healthz"),
            probe_health(client, "http://127.0.0.1:7115", "/healthz"),
            probe_health(client, "http://127.0.0.1:7116", "/healthz"),
            probe_health(client, "http://127.0.0.1:7117", "/healthz"),
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
                "Workshop / Registry",
                registry,
                "not reported",
                Some("/admin/workshop"),
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
                "not reported",
                Some("/admin/build-cache"),
            ),
        ];
    }

    let (platform, homepage, docs, identity, diagnostics, registry, artifacts, build_cache) = tokio::join!(
        probe_health(client, origin, "/api/admin/platform-healthz"),
        probe_health(client, origin, "/healthz"),
        probe_health(client, origin, "/docs/healthz"),
        probe_health(client, origin, "/api/identity/v1/status"),
        probe_health(client, origin, "/api/diagnostics/healthz"),
        probe_health(client, origin, "/api/registry/healthz"),
        probe_health(client, origin, "/api/updates/healthz"),
        probe_health(client, origin, "/api/build-cache/healthz"),
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
            "Workshop / Registry",
            registry,
            "not reported",
            Some("/admin/workshop"),
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
            "not reported",
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

fn service(
    id: &'static str,
    name: &'static str,
    health: &'static str,
    readiness: &'static str,
    admin_url: Option<&'static str>,
) -> ServiceSummary {
    ServiceSummary {
        id,
        name,
        health,
        readiness,
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

    // The Platform installs this middleware around the composed service
    // routers. Exercise the same public admin surface in route-level tests.
    fn admin_app() -> Router {
        super::router().layer(axum::middleware::from_fn(super::shell_middleware))
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
        let status = if uri.path() == "/docs/healthz" {
            StatusCode::SERVICE_UNAVAILABLE
        } else {
            StatusCode::OK
        };
        (status, "private-probe-payload").into_response()
    }
    use tower::ServiceExt;

    async fn fake_health(uri: Uri) -> axum::response::Response {
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
            .layer(axum::middleware::from_fn(super::shell_middleware))
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
        let document = super::render_admin_fragment_document(
            "<h1>Accessible page</h1>",
            "/admin/operations",
        )
        .unwrap();
        assert!(document.contains("href=\"#admin-content\">Skip to content</a>"));
        assert!(document.contains("<main id=\"admin-content\" tabindex=\"-1\">"));
    }

    #[tokio::test]
    async fn history_filters_stack_before_their_minimum_width_overflows() {
        let response = super::stylesheet().await;
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let css = std::str::from_utf8(&body).unwrap();
        assert!(css.contains("@media(max-width:620px) { .history-controls { grid-template-columns:1fr; }"));
        assert!(css.contains("@media(max-width:760px) { .history-workbench { grid-template-columns:1fr; }"));
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
            .layer(axum::middleware::from_fn(super::shell_middleware))
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
            .layer(axum::middleware::from_fn(super::shell_middleware))
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
            .layer(axum::middleware::from_fn(super::shell_middleware))
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
            .layer(axum::middleware::from_fn(super::shell_middleware))
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

        assert!(html.contains("href=\"/admin/workshop\""));
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
            "data-deployment-targets=\"atlas,homepage,docs,identity,diagnostics,registry,artifact,build-cache\""
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
    async fn public_shell_contains_no_operator_data() {
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
        assert!(html.contains("Identity list is currently limited to the newest 200 requests"));
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
        assert!(script.contains("'/api/identity/v1/admin/permission-requests'"));
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
        assert!(script.contains("pending.filter(item => !isExpiredByTime(item))"));
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
        assert!(document.contains("Sign-in required"));
        assert!(document.contains("/login"));
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
                "Sign-in required",
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
        assert!(script.contains("source.kind !== 'Platform operation' && item.status === 'pending'"));
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
        assert!(script.contains("const matchesFilter = item =>"));
        assert!(script.contains("No response received. Refresh to verify whether access was revoked."));
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
        assert!(
            script.contains("${lifecycleLabel || dispatchLabels[item.dispatch_state] || 'Stage not reported'}")
        );
        assert!(script.contains("expired: 'Expired before worker claim'"));
        assert!(script.contains("claimed: 'Worker claimed deployment'"));
        assert!(script.contains("completed: 'Worker finished deployment'"));
        assert!(script.contains("Worker stopped; outcome unknown"));
        assert!(script.contains("secondaryLabel: 'Dispatch needs attention'"));
        assert!(script.contains("Approval saved; deployment is queued for the worker."));
        assert!(
            script.contains("Approval saved; the worker claimed the deployment and it is running.")
        );
        assert!(script.contains("Approval saved; deployment succeeded."));
        assert!(script
            .contains("dispatch_attempt_superseded: 'was superseded by a newer request state'"));
        assert!(script.contains(
            "const link = document.createElement('a'); link.href = '/admin/deployments';"
        ));
        assert!(script.contains("const validDeploymentSelection = item =>"));
        assert!(script.contains("const isCurrentDeployment = item => ['approved', 'executing'].includes(item.status);"));
        assert!(script.contains(".filter(item => item && isCurrentDeployment(item))"));
        assert!(script.contains("No active deployments."));
        assert!(script.contains("serviceDeployTargets.has(item.service_target)"));
        assert!(script.contains("document.querySelector('[data-deployment-targets]')"));
        assert!(script.contains("Target: ${deploymentSelectionLabel(item)}"));
        assert!(script.contains("const canRetry = validDeploymentSelection(item) && item.status === 'approved'"));
        assert!(script.contains("&& item.retry_eligible && ['failed', 'unknown'].includes(item.dispatch_state)"));
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
        let deployments = script
            .split("const deployRequests = document.querySelector('#deploy-requests');")
            .nth(1)
            .unwrap()
            .split("const sshAccessList =")
            .next()
            .unwrap();
        assert!(deployments.contains(".filter(item => item && isCurrentDeployment(item))"));
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
        assert!(approvals.contains("Approve and queue"));
        assert!(approvals.contains("Approve selected access"));
        assert!(approvals.contains("approvals-more-deploy"));
        assert!(approvals.contains("approvals-more-ssh"));
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
        assert_eq!(health_requests.load(Ordering::SeqCst), 8);
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
        let serialized = serde_json::to_string(&services).unwrap();
        assert!(!serialized.contains("private-probe-payload"));
        server.abort();
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
