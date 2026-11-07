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
        group: "Overview",
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
        group: "Catalog",
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
        group: "Distribution",
        name: "Artifacts",
        path: Some("/admin/artifacts"),
        description: "Signed Vapor releases",
        service_ids: &["artifacts"],
        state: SectionState::Implemented,
    },
    AdminSection {
        name: "Build Cache",
        group: "Distribution",
        path: Some("/admin/build-cache"),
        description: "Per-target compiler snapshots",
        service_ids: &["build-cache"],
        state: SectionState::Implemented,
    },
    AdminSection {
        group: "Distribution",
        name: "Native builders",
        path: None,
        description: "VPS-managed Linux and Windows build orchestration is not available yet",
        service_ids: &[],
        state: SectionState::Planned,
    },
    AdminSection {
        group: "Operations",
        name: "Approvals",
        path: Some("/admin/approvals"),
        description: "Pending decisions from Platform and Identity",
        service_ids: &["platform", "identity"],
        state: SectionState::Implemented,
    },
    AdminSection {
        group: "Operations",
        name: "Deployments",
        path: Some("/admin/deployments"),
        description: "Deployment lifecycle and execution stages",
        service_ids: &["platform"],
        state: SectionState::Implemented,
    },
    AdminSection {
        group: "Operations",
        name: "SSH Access",
        path: Some("/admin/ssh-access"),
        description: "Temporary access grants and active sessions",
        service_ids: &["platform"],
        state: SectionState::Implemented,
    },
    AdminSection {
        group: "Operations",
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
        group: "Operations",
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
.service-grid { display:grid; grid-template-columns:repeat(auto-fit,minmax(220px,1fr)); gap:10px; }
.service-card { background:#182330; border:1px solid #2b3a4b; border-radius:6px; padding:12px; }
.service-card h2 { font-size:15px; margin:0 0 8px; }
.service-state { font-weight:650; }
.state-healthy { color:#6fc58a; }
.state-unavailable { color:#ed7a80; }
.state-planned { color:#d4bd69; }
main { padding:30px clamp(20px,4vw,56px); max-width:1100px; }
.admin-workspace { min-width:0; }
.admin-header { display:flex; align-items:center; justify-content:space-between; gap:18px; padding:14px clamp(20px,4vw,56px); border-bottom:1px solid #2b3a4b; background:#151f2a; }
.admin-section-name { margin:2px 0 0; font-size:16px; font-weight:650; }
.admin-statuses { display:flex; align-items:center; justify-content:flex-end; gap:8px 18px; flex-wrap:wrap; }
.admin-status-item { margin:0; color:#9eacba; font-size:12px; }
.operator-actions { margin-top:18px; }
.action-links { display:flex; flex-wrap:wrap; gap:8px; }
.action-links a { padding:7px 10px; border:1px solid #40536a; border-radius:5px; color:#c7d2de; text-decoration:none; }
.action-links a:hover { background:#26394e; color:#fff; }
.status-pill { display:inline-block; margin-left:5px; padding:2px 7px; border:1px solid #40536a; border-radius:10px; color:#c7d2de; font-size:11px; }
.status-pill.status-healthy { border-color:#316947; color:#8bd59f; }
.status-pill.status-unavailable { border-color:#7a3e45; color:#f08b92; }
.status-pill.status-protected { border-color:#53677e; color:#b4c2d1; }
.history-controls { display:grid; grid-template-columns:minmax(220px,2fr) repeat(2,minmax(150px,1fr)); gap:10px; align-items:end; margin:12px 0; }
.history-controls label { margin:0; }
.history-count { margin:8px 0 12px; }
.history-toolbar { display:flex; justify-content:space-between; align-items:center; gap:10px; flex-wrap:wrap; margin:10px 0; }
.history-toolbar p { margin:0; }
.history-list { display:grid; gap:8px; }
.request-queue-grid { display:grid; grid-template-columns:repeat(auto-fit,minmax(210px,1fr)); gap:10px; margin-top:12px; }
.request-queue-card { min-width:0; background:#182330; border:1px solid #2b3a4b; border-radius:6px; padding:12px; }
.request-queue-card h3 { margin:0 0 8px; font-size:14px; }
.request-queue-count { margin:0; color:#e6edf3; font-size:22px; font-weight:700; }
.request-queue-label { margin:2px 0 8px; color:#9eacba; font-size:12px; }
.request-queue-card a { font-size:12px; }
.request-queue-header { display:flex; align-items:center; justify-content:space-between; gap:10px; }
.request-queue-header h2 { margin:0; }
.history-entry { background:#182330; border:1px solid #2b3a4b; border-radius:6px; padding:12px; }
.history-entry header { display:flex; align-items:flex-start; justify-content:space-between; gap:10px; }
.history-entry h3 { margin:0; font-size:14px; overflow-wrap:anywhere; }
.history-meta { display:flex; flex-wrap:wrap; gap:6px 12px; margin:6px 0; color:#9eacba; font-size:12px; }
.history-kind { color:#8fc5f4; font-size:11px; font-weight:650; text-transform:uppercase; letter-spacing:.06em; }
.history-state { border:1px solid #40536a; border-radius:10px; padding:1px 7px; white-space:nowrap; font-size:11px; }
.history-state.state-good { border-color:#316947; color:#8bd59f; }
.history-state.state-warning { border-color:#806a2f; color:#e5cb78; }
.approval-state-warning { color:#e5cb78; }
.history-state.state-danger { border-color:#7a3e45; color:#f08b92; }
.history-detail { margin:7px 0 0; overflow-wrap:anywhere; }
.history-detail-list { margin:6px 0 0; padding-left:18px; color:#c7d2de; }
.table-scroll { max-width:100%; overflow-x:auto; }
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
@media(max-width:720px) { .shell { grid-template-columns:1fr; } .shell > nav { border-right:0; border-bottom:1px solid #2b3a4b; padding:12px; } .nav-group { display:inline-block; margin:8px 8px 4px; } .shell > nav a { display:inline-flex; } .admin-header { align-items:flex-start; flex-direction:column; padding:12px 14px; } .admin-statuses { justify-content:flex-start; } main { padding:20px 14px; } }
@media(max-width:560px) { .history-controls { grid-template-columns:1fr; } .history-entry header { flex-direction:column; } }
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
        const revision = document.createElement('p');
        revision.className = 'muted';
        revision.textContent = `Platform revision: ${body.deployed_revision || 'not recorded'} · ${body.state || 'unknown'}`;
        overview.append(revision);
        const grid = document.createElement('div');
        grid.className = 'service-grid';
        for (const service of (body && Array.isArray(body.services) ? body.services : [])) {
          const card = document.createElement('section');
          card.className = 'service-card';
          const heading = document.createElement('h2');
          heading.textContent = service.name;
          const health = document.createElement('p');
          health.className = `service-state state-${service.health}`;
          health.textContent = `Health: ${service.health}`;
          const readiness = document.createElement('p');
          readiness.className = 'muted';
          readiness.textContent = `Feature readiness: ${service.readiness}`;
          card.append(heading, health, readiness);
          if (service.admin_url) {
            const link = document.createElement('a');
            link.href = service.admin_url;
            link.textContent = 'Open section';
            card.append(link);
          }
          grid.append(card);
        }
        overview.append(grid);
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
      const grid = document.createElement('div'); grid.className = 'request-queue-grid';
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
        const card = document.createElement('section'); card.className = 'request-queue-card';
        const heading = document.createElement('h3'); heading.textContent = outcome.source.name; card.append(heading);
        if (outcome.error) {
          const status = document.createElement('p'); status.className = 'error'; status.textContent = outcome.error; card.append(status);
        } else {
          const primary = document.createElement('p'); primary.className = 'request-queue-count'; primary.textContent = String(outcome.counts.primary);
          const primaryLabel = document.createElement('p'); primaryLabel.className = 'request-queue-label'; primaryLabel.textContent = outcome.counts.label;
          card.append(primary, primaryLabel);
          if (outcome.counts.secondary > 0) {
            const secondary = document.createElement('p'); secondary.className = 'muted'; secondary.textContent = `${outcome.counts.secondary} · ${outcome.counts.secondaryLabel}`; card.append(secondary);
          }
        }
        const link = document.createElement('a'); link.href = outcome.source.href; link.textContent = 'Open owner workflow'; card.append(link);
        grid.append(card);
      }
      requestQueue.replaceChildren(grid);
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
    const sourceDefinitions = [
      { key: 'deploy', url: '/api/admin/deploy-requests?limit=200', label: 'Platform deployment approvals' },
      { key: 'ssh', url: '/api/admin/ssh-access/requests?limit=200', label: 'Platform SSH access requests' },
      { key: 'identity', url: '/api/identity/v1/admin/permission-requests', label: 'Identity permission requests' }
    ];
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
    const humanTime = value => {
      const seconds = Number(value);
      return Number.isFinite(seconds) && seconds > 0 && seconds <= 8640000000000
        ? new Date(seconds * 1000).toLocaleString() : 'not recorded';
    };
    const serviceDeployTargets = new Set(['atlas', 'homepage', 'docs', 'identity', 'diagnostics', 'registry', 'artifact', 'build-cache']);
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
      appendText(card, 'p', `Submitted by ${item.requester_kind || 'unknown'} · profile ${item.requester_profile_id || 'unknown'} · expires ${humanTime(item.expires_at_unix)}`, 'muted');
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
      appendText(card, 'h3', `${item.fingerprint || 'Unknown key'} · ${item.requested_capabilities?.join(', ') || 'shell only'}`);
      appendText(card, 'p', item.purpose || 'No purpose provided.');
      appendText(card, 'p', `Requested by ${item.requester_kind || 'unknown'} · profile ${item.requester_profile_id || 'unknown'} · requested ${humanTime(item.created_at_unix)} · duration ${item.requested_duration_seconds || 0}s`, 'muted');
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
        label.append(input, document.createTextNode(` ${capability}${capability === 'host_admin' ? ' (full temporary sudo)' : ''}`));
        fieldset.append(label);
      }
      const durationLabel = document.createElement('label'); durationLabel.textContent = 'Granted duration in seconds ';
      const duration = document.createElement('input'); duration.type = 'number'; duration.min = '1'; duration.max = String(8 * 60 * 60);
      duration.value = String(Math.min(Number(item.requested_duration_seconds) || 3600, 8 * 60 * 60));
      duration.required = true; durationLabel.append(duration);
      const reviewLabel = document.createElement('label'); const review = document.createElement('input'); review.type = 'checkbox'; review.required = true;
      reviewLabel.append(review, document.createTextNode(' I reviewed the key, purpose, and actual grant'));
      const hostLabel = document.createElement('label'); const hostConfirm = document.createElement('input'); hostConfirm.type = 'checkbox';
      hostLabel.append(hostConfirm, document.createTextNode(' I approve unrestricted root access and understand expiry cleanup can be disabled'));
      const approve = document.createElement('button'); approve.type = 'submit'; approve.textContent = 'Approve selected access';
      form.append(fieldset, durationLabel, reviewLabel, hostLabel, approve);
      form.addEventListener('submit', event => {
        event.preventDefault();
        if (!review.checked || !duration.reportValidity()) return;
        const granted = choices.filter(input => input.checked).map(input => input.value);
        if (granted.includes('host_admin') && (granted.length !== 1 || !hostConfirm.checked)) {
          feedback.textContent = 'Full temporary sudo requires its separate confirmation and cannot be combined with other capabilities.';
          return;
        }
        submitDecision(approve, 'ssh', item, 'approve', { expected_version: item.version, confirmed: true,
          host_admin_confirmed: hostConfirm.checked, granted_capabilities: granted,
          granted_duration_seconds: Number(duration.value) });
      });
      card.append(form);
      card.append(confirmationForm(item, 'I intend to reject this access request', 'Reject', button => submitDecision(button, 'ssh', item, 'reject', { expected_version: item.version, confirmed: true })));
    };
    const renderIdentity = (item, card) => {
      appendText(card, 'h3', `${item.action || 'permission change'} · ${item.role || 'unknown role'}`);
      appendText(card, 'p', `Steam ${item.steam_id64 || 'not linked'} · GitHub ${item.github_login || 'not linked'}`);
      appendText(card, 'p', `Submitted by ${item.requester_kind || 'unknown'} · profile ${item.requester_profile_id || 'unknown'} · expires ${humanTime(item.expires_at_unix)}`, 'muted');
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
      appendText(card, 'p', `Submitted by ${item.requester_kind || 'unknown'} · profile ${item.requester_profile_id || 'unknown'} · expired ${humanTime(item.expires_at_unix)}`, 'muted');
      appendText(card, 'p', 'Expired by time · Identity has not yet recorded the transition', 'approval-state-warning');
    };
    const refreshSource = async source => {
      const container = containers[source.key];
      container.textContent = 'Loading…';
      try {
        const response = await fetch(source.url, { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(6000) });
        const items = await response.json().catch(() => null);
        if (!response.ok) {
          container.replaceChildren();
          appendText(container, 'p', response.status === 401 ? 'Sign in required.' : response.status === 403 ? 'Root access required.' : `Unavailable (HTTP ${response.status}).`, 'error');
          return;
        }
        const records = Array.isArray(items) ? items : items?.items;
        if (!Array.isArray(records)) throw new Error('invalid response');
        const pending = records.filter(item => item && item.status === 'pending');
        const expiredIdentity = source.key === 'identity' ? pending.filter(isExpiredByTime) : [];
        const actionable = source.key === 'identity'
          ? pending.filter(item => !isExpiredByTime(item))
          : pending;
        container.replaceChildren();
        if (!pending.length) { appendText(container, 'p', 'No pending requests.', 'muted'); return; }
        if (!actionable.length && !expiredIdentity.length) {
          appendText(container, 'p', 'No pending requests.', 'muted');
          return;
        }
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
      } catch (_) {
        container.replaceChildren();
        appendText(container, 'p', 'This request source is temporarily unavailable or returned an invalid response.', 'error');
      }
    };
    const refreshInbox = async () => {
      if (refreshButton?.disabled) return;
      if (refreshButton) { refreshButton.disabled = true; refreshButton.textContent = 'Refreshing…'; }
      await Promise.all(sourceDefinitions.map(refreshSource));
      if (refreshButton) { refreshButton.disabled = false; refreshButton.textContent = 'Refresh'; }
    };
    refreshButton?.addEventListener('click', refreshInbox);
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
    let failures = [];
    let sourceStates = [];
    const addRecord = (source, item) => {
      if (!item || typeof item !== 'object') return;
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
    const renderHistory = () => {
      const query = (search?.value || '').trim().toLocaleLowerCase();
      const selectedKind = kindFilter?.value || 'all';
      const selectedStatus = statusFilter?.value || 'all';
      const visible = records.filter(record =>
        (selectedKind === 'all' || record.kind === selectedKind) &&
        (selectedStatus === 'all' || record.statusKey === selectedStatus || record.dispatchStatusKey === selectedStatus) &&
        (!query || `${record.kind} ${record.statusLabel} ${record.id} ${record.search}`.toLocaleLowerCase().includes(query))
      );
      operationHistory.replaceChildren();
      count.textContent = `Showing ${visible.length} of ${records.length} loaded records.`;
      if (!records.length && failures.length === sources.length) {
        operationHistory.textContent = 'History could not be loaded from any owner service.';
        return;
      }
      if (!visible.length) {
        operationHistory.textContent = records.length ? 'No records match these filters.' : 'No records have been recorded by the available services.';
        return;
      }
      const list = document.createElement('div');
      list.className = 'history-list';
      for (const record of visible) {
        const card = document.createElement('article');
        card.className = 'history-entry';
        const header = document.createElement('header');
        const heading = document.createElement('h3'); heading.textContent = record.title;
        const state = document.createElement('span'); state.className = 'history-state'; state.textContent = record.statusLabel;
        if (record.status === 'failed' || record.status === 'rejected' || record.status === 'expired' || record.status === 'cancelled' || record.status === 'revoked' || record.dispatchStatusKey === 'dispatch-failed' || record.dispatchStatusKey === 'dispatch-unknown') state.classList.add('state-danger');
        else if (record.status === 'success' || record.status === 'succeeded' || record.status === 'active') state.classList.add('state-good');
        else if (record.status === 'pending' || record.status === 'approved' || record.status === 'executing' || record.status === 'provisioning' || record.status === 'stale' || record.statusKey === 'expired-pending' || record.dispatchStatusKey === 'dispatch-dispatching' || record.dispatchStatusKey === 'dispatch-queued') state.classList.add('state-warning');
        header.append(heading, state);
        const meta = document.createElement('p'); meta.className = 'history-meta';
        const kind = document.createElement('span'); kind.className = 'history-kind'; kind.textContent = record.kind;
        const date = document.createElement('time'); date.dateTime = isoTime(record.timestamp);
        date.textContent = `${record.timestampLabel}: ${humanTime(record.timestamp)}`;
        const id = document.createElement('span'); id.textContent = `${record.kind === 'Platform operation' ? 'Operation' : 'Request'} ${record.id}`;
        meta.append(kind, date, id);
        card.append(header, meta);
        if (record.subject) { const subject = document.createElement('p'); subject.className = 'history-detail'; subject.textContent = record.subject; card.append(subject); }
        if (record.details.length) {
          const detailList = document.createElement('ul'); detailList.className = 'history-detail-list';
          for (const detail of record.details) { const line = document.createElement('li'); line.textContent = detail; detailList.append(line); }
          card.append(detailList);
        }
        if (record.link) { const link = document.createElement('a'); link.href = record.link; link.textContent = record.kind === 'Deployment' ? 'Open CI run' : 'Open owning section'; link.rel = 'noopener noreferrer'; card.append(link); }
        if (record.eventsUrl) {
          const details = document.createElement('details');
          const summary = document.createElement('summary'); summary.textContent = 'Request activity';
          const body = document.createElement('div'); body.className = 'history-detail'; body.textContent = 'Open to load owner-recorded status changes.';
          details.append(summary, body);
          details.addEventListener('toggle', async () => {
            if (!details.open || details.dataset.loaded === 'true') return;
            body.textContent = 'Loading activity…';
            try {
              const response = await fetch(record.eventsUrl, { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(6000) });
              const events = await response.json().catch(() => null);
              if (!response.ok || !Array.isArray(events)) { body.textContent = response.status === 403 ? 'Root access required.' : `Activity unavailable (HTTP ${response.status}).`; return; }
              body.replaceChildren();
              if (!events.length) body.textContent = 'No request transitions were recorded.';
              for (const event of events) {
                const line = document.createElement('p'); line.className = 'history-detail';
                const transition = event.from_status ? `${event.from_status} → ${event.to_status}` : event.to_status;
                line.textContent = `${humanTime(Number(event.at_unix))} · ${transition} · ${safeText(event.actor_label || event.actor_kind)}`;
                body.append(line);
              }
              details.dataset.loaded = 'true';
            } catch (_) { body.textContent = 'Could not contact the Platform history API.'; }
          });
          card.append(details);
        }
        list.append(card);
      }
      operationHistory.append(list);
    };
    const refreshButton = document.querySelector('#history-refresh');
    const loadMoreButton = document.querySelector('#history-load-more');
    const renderSourceStatus = () => {
      sourceStatus.replaceChildren();
      for (const state of sourceStates) {
        const line = document.createElement('p');
        line.className = state.error ? 'error' : 'muted';
        const summary = state.error || `${state.loaded} loaded${state.nextCursor ? ' · more available' : ''}`;
        line.textContent = `${state.source.label}: ${summary}`;
        sourceStatus.append(line);
      }
      const refreshed = document.createElement('span');
      refreshed.className = 'muted';
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
      const items = Array.isArray(requests) ? requests : (Array.isArray(requests?.items) ? requests.items : []);
      if (!items.length) {
        deployRequests.textContent = 'No deploy requests have been recorded.';
        return;
      }
      for (const item of items) {
        const card = document.createElement('article');
        card.className = 'card';
        const heading = document.createElement('h3');
        heading.textContent = `${item.repository} · ${item.branch} · ${item.status}`;
        const requester = document.createElement('p');
        requester.className = 'muted';
        requester.textContent = `Submitted by ${item.requester_kind || 'unknown'} · profile ${item.requester_profile_id || 'unknown'}`;
        const decisionStage = document.createElement('p');
        decisionStage.className = 'muted';
        decisionStage.textContent = `Decision: ${item.status === 'pending' ? 'awaiting review' : item.status === 'rejected' ? 'rejected' : item.status === 'expired' ? 'expired' : 'approved and saved'}`;
        const dispatchStage = document.createElement('p');
        dispatchStage.className = 'muted';
        const dispatchLabels = {
          not_attempted: 'not attempted', dispatching: 'dispatch in progress',
          queued: 'queued for worker', claimed: 'worker claimed deployment',
          completed: 'worker finished deployment', expired: 'expired before worker claim',
          failed: 'dispatch or worker claim failed', unknown: 'dispatch result unknown'
        };
        const selectionValid = validDeploymentSelection(item);
        const retryLabel = selectionValid && item.retry_eligible && ['failed', 'unknown'].includes(item.dispatch_state)
          ? '; retry available' : '';
        const lifecycleLabel = item.status === 'executing' ? 'worker claimed; deployment running' :
          item.status === 'succeeded' ? 'deployment succeeded' :
          item.status === 'failed' && item.dispatch_state === 'unknown' ? 'worker stopped; deployment outcome unknown' :
          item.status === 'failed' ? 'deployment failed' :
          item.status === 'expired' ? 'expired before worker claim' : null;
        dispatchStage.textContent = `Deployment: ${lifecycleLabel || dispatchLabels[item.dispatch_state] || 'not reported'}${retryLabel}${item.dispatch_attempts ? ` · attempt ${item.dispatch_attempts}` : ''}${item.dispatch_detail && !lifecycleLabel ? ` · ${String(item.dispatch_detail).slice(0, 240)}` : ''}`;
        const revision = document.createElement('p');
        revision.textContent = `Commit ${item.commit} · tree ${item.tree}`;
        const target = document.createElement('p');
        target.className = selectionValid ? 'muted' : 'error';
        target.textContent = `Target: ${deploymentSelectionLabel(item)}`;
        const pins = document.createElement('p');
        pins.className = 'muted';
        pins.textContent = `${Array.isArray(item.submodules) ? item.submodules.length : 0} pinned service revisions · expires ${item.expires_at_unix}`;
        const summary = document.createElement('p');
        summary.textContent = item.summary;
        const workflow = document.createElement('a');
        workflow.href = item.workflow_url;
        workflow.rel = 'noopener noreferrer';
        workflow.textContent = 'Open successful CI run';
        card.append(heading, requester, decisionStage, dispatchStage, revision, target, pins, summary, workflow);
        if (actionFeedback.has(item.id)) {
          const feedback = document.createElement('p');
          feedback.className = item.status === 'approved' && ['failed', 'unknown'].includes(item.dispatch_state) ? 'error' : 'muted';
          feedback.textContent = actionFeedback.get(item.id);
          card.append(feedback);
        }
        if (item.result_detail) {
          const outcome = document.createElement('p');
          outcome.className = 'muted';
          outcome.textContent = item.result_detail;
          card.append(outcome);
        }
        const details = document.createElement('details');
        const summaryLabel = document.createElement('summary');
        summaryLabel.textContent = 'Pinned submodules';
        const list = document.createElement('ul');
        for (const pin of (Array.isArray(item.submodules) ? item.submodules : [])) {
          const line = document.createElement('li');
          line.textContent = `${pin.path}: ${pin.commit}`;
          list.append(line);
        }
        details.append(summaryLabel, list);
        card.append(details);
        const pendingActions = selectionValid ? ['approve', 'reject'] : ['reject'];
        for (const decision of (item.status === 'pending' ? pendingActions : selectionValid && item.status === 'approved' && item.retry_eligible ? ['dispatch'] : item.status === 'executing' ? ['reconcile'] : [])) {
          const form = document.createElement('form');
          form.className = 'inline';
          const label = document.createElement('label');
          const confirmed = document.createElement('input');
          confirmed.type = 'checkbox';
          confirmed.required = true;
          const wording = decision === 'approve' ? 'I reviewed this exact revision' : decision === 'dispatch' ? 'Queue this already-approved revision now' : decision === 'reconcile' ? 'I confirmed this worker is no longer running; mark this execution failed without retrying it' : 'I intend to reject this request';
          label.append(confirmed, document.createTextNode(` ${wording}`));
          const button = document.createElement('button');
          button.type = 'submit';
          button.textContent = decision === 'approve' ? 'Approve deploy' : decision === 'dispatch' ? 'Retry deploy dispatch' : decision === 'reconcile' ? 'Reconcile stopped worker' : 'Reject';
          form.append(label, button);
          form.addEventListener('submit', async event => {
            event.preventDefault();
            if (!confirmed.checked) return;
            button.disabled = true;
            try {
              const result = await fetch(`/api/admin/deploy-requests/${encodeURIComponent(item.id)}/${decision}`, {
                method: 'POST', credentials: 'same-origin', cache: 'no-store',
                headers: { 'content-type': 'application/json' },
                body: JSON.stringify(['dispatch', 'reconcile'].includes(decision)
                  ? { expected_version: item.version, confirmed: true }
                  : { expected_version: item.version, confirmed: true,
                      reason: decision === 'approve' ? 'approved in Platform Operations' : 'rejected in Platform Operations' })
              });
              const body = await result.clone().json().catch(() => null);
              const plainText = body ? '' : await result.text().catch(() => '');
              const decisionSaved = body?.error?.decision_saved === true;
              const dispatchState = body?.error?.code === 'dispatch_result_unknown' ? 'dispatch outcome is unknown' :
                body?.error?.code === 'dispatch_failed' ? 'dispatch failed' : 'the next deployment stage did not complete';
              const savedStage = decisionSaved
                ? `Approval saved, but ${dispatchState}: ${body.error.message || 'no further detail was reported.'}${body.error.retry_eligible ? ' A retry is available.' : ''}`
                : '';
              const message = savedStage || body?.error?.message || plainText.trim().slice(0, 240) || (result.ok ? 'Action completed.' : `Action failed (${result.status}).`);
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

  const sshAccess = document.querySelector('#ssh-access');
  if (sshAccess) {
    const actionFeedback = new Map();
    const submitSshAction = async (button, itemId, path, body, fallback) => {
      button.disabled = true;
      try {
        const result = await fetch(path, {
          method: 'POST', credentials: 'same-origin', cache: 'no-store',
          headers: { 'content-type': 'application/json' }, body: JSON.stringify(body)
        });
        if (result.ok) actionFeedback.delete(itemId);
        else actionFeedback.set(itemId, await responseFailure(result, fallback));
      } catch (_) {
        actionFeedback.set(itemId, 'No response was received; the request may have completed. The current status has been refreshed; inspect access history before retrying.');
      }
      await refresh();
    };
    const refresh = async () => {
      let response;
      let items;
      try {
        response = await fetch('/api/admin/ssh-access/requests?limit=200', { credentials: 'same-origin', cache: 'no-store' });
      } catch (_) { sshAccess.textContent = 'Could not contact the temporary SSH service.'; return; }
      sshAccess.replaceChildren();
      if (!response.ok) {
        await showResponseFailure(sshAccess, response, 'Could not load temporary SSH requests');
        return;
      }
      const body = await response.json().catch(() => null);
      items = Array.isArray(body) ? body : body?.items;
      if (!Array.isArray(items)) { sshAccess.textContent = 'The temporary SSH service returned an invalid response.'; return; }
      if (!items.length) { sshAccess.textContent = 'No temporary SSH requests.'; return; }
      for (const item of items) {
        const card = document.createElement('article'); card.className = 'card';
        const title = document.createElement('h3'); title.textContent = `${item.status} · ${item.fingerprint}`;
        const purpose = document.createElement('p'); purpose.textContent = item.purpose;
        const requester = document.createElement('p'); requester.className = 'muted';
        requester.textContent = `Submitted by ${item.requester_kind || 'unknown'} · profile ${item.requester_profile_id || 'unknown'}`;
        const scope = document.createElement('p'); scope.className = 'muted';
        scope.textContent = `Requested: ${item.requested_capabilities.join(', ') || 'shell only'} · ${item.requested_duration_seconds}s`;
        card.append(title, purpose, requester, scope);
        if (actionFeedback.has(item.id)) {
          const feedback = document.createElement('p');
          feedback.className = 'error';
          feedback.setAttribute('role', 'alert');
          feedback.textContent = actionFeedback.get(item.id);
          card.append(feedback);
        }
        if (item.detail) {
          const detail = document.createElement('p'); detail.className = 'muted'; detail.textContent = item.detail;
          card.append(detail);
        }
        if (item.expires_at_unix) {
          const expiry = document.createElement('p'); expiry.className = 'muted';
          expiry.textContent = `${item.status === 'active' ? 'Expires' : 'Approved expiry'}: ${new Date(item.expires_at_unix * 1000).toLocaleString()}`;
          card.append(expiry);
        }
        const history = document.createElement('details');
        const historyLabel = document.createElement('summary'); historyLabel.textContent = 'Access history';
        const historyBody = document.createElement('div'); historyBody.className = 'muted';
        history.append(historyLabel, historyBody);
        history.addEventListener('toggle', async () => {
          if (!history.open || history.dataset.loaded === 'true') return;
          historyBody.textContent = 'Loading request history…';
          try {
            const eventsResponse = await fetch(`/api/admin/ssh-access/requests/${encodeURIComponent(item.id)}/events`, {
              credentials: 'same-origin', cache: 'no-store'
            });
            if (!eventsResponse.ok) {
              await showResponseFailure(historyBody, eventsResponse, 'Could not load request history');
              return;
            }
            const events = await eventsResponse.json().catch(() => null);
            if (!Array.isArray(events)) { historyBody.textContent = 'The SSH service returned an invalid history response.'; return; }
            historyBody.replaceChildren();
            if (!events.length) { historyBody.textContent = 'No access history recorded.'; }
            for (const event of events) {
              const line = document.createElement('p');
              const transition = event.from_status ? `${event.from_status} → ${event.to_status}` : event.to_status;
              const actor = `${event.actor_kind || 'actor'} · ${event.actor_label || event.actor_profile_id || 'unknown'}`;
              const timestamp = Number.isFinite(event.at_unix) ? new Date(event.at_unix * 1000).toLocaleString() : 'unknown time';
              line.textContent = `${timestamp} · ${transition} · ${actor}`;
              historyBody.append(line);
            }
            history.dataset.loaded = 'true';
          } catch (_) { historyBody.textContent = 'Could not contact the temporary SSH service.'; }
        });
        card.append(history);
        if (item.status === 'active' && item.username) {
          const login = document.createElement('p');
          login.textContent = `SSH account: ${item.username}@${item.host} · port 22 · key ${item.fingerprint}`;
          card.append(login);
        }
        if (item.granted_duration_seconds != null) {
          const granted = document.createElement('p'); granted.className = 'muted';
          granted.textContent = `Granted: ${item.granted_capabilities.join(', ') || 'shell only'} · ${item.granted_duration_seconds}s`;
          card.append(granted);
        }
        if (item.status === 'pending') {
          const form = document.createElement('form'); const fields = document.createElement('fieldset');
          const legend = document.createElement('legend'); legend.textContent = 'Actual grant'; fields.append(legend);
          const choices = [];
          for (const value of ['vapor_deploy', 'vapor_service_control', 'host_admin']) {
            const label = document.createElement('label'); const input = document.createElement('input');
            input.type = 'checkbox'; input.value = value; input.checked = item.requested_capabilities.includes(value); choices.push(input);
            label.append(input, document.createTextNode(` ${value}${value === 'host_admin' ? ' (full temporary sudo)' : ''}`)); fields.append(label);
          }
          const durationLabel = document.createElement('label'); durationLabel.textContent = 'Duration in seconds';
          const duration = document.createElement('input'); duration.type = 'number'; duration.min = '1';
          duration.max = String(8 * 60 * 60); duration.value = String(item.requested_duration_seconds); durationLabel.append(duration);
          const adjustment = document.createElement('p'); adjustment.className = 'muted';
          adjustment.textContent = 'Set the actual grant here. You may expand or reduce the requested scope and duration within server policy.';
          fields.append(adjustment);
          const confirmLabel = document.createElement('label'); const confirm = document.createElement('input'); confirm.type = 'checkbox'; confirm.required = true;
          confirmLabel.append(confirm, document.createTextNode(' I reviewed the key, purpose, and actual grant'));
          const adminLabel = document.createElement('label'); const adminConfirm = document.createElement('input'); adminConfirm.type = 'checkbox';
          adminLabel.append(adminConfirm, document.createTextNode(' I approve unrestricted root access and understand it can disable its own expiry cleanup'));
          const button = document.createElement('button'); button.type = 'submit'; button.textContent = 'Approve temporary access';
          form.append(fields, durationLabel, confirmLabel, adminLabel, button);
          form.addEventListener('submit', async event => {
            event.preventDefault(); if (!confirm.checked) return;
            const selected = choices.filter(input => input.checked).map(input => input.value);
            if (selected.includes('host_admin') && (selected.length !== 1 || !adminConfirm.checked)) return;
            await submitSshAction(button, item.id,
              `/api/admin/ssh-access/requests/${encodeURIComponent(item.id)}/approve`,
              { expected_version: item.version, confirmed: true, host_admin_confirmed: adminConfirm.checked,
                granted_capabilities: selected, granted_duration_seconds: Number(duration.value) },
              'Grant failed');
          });
          card.append(form);
          const reject = document.createElement('button'); reject.type = 'button'; reject.textContent = 'Reject request';
          reject.addEventListener('click', async () => {
            await submitSshAction(reject, item.id,
              `/api/admin/ssh-access/requests/${encodeURIComponent(item.id)}/reject`,
              { expected_version: item.version, confirmed: true }, 'Rejection failed');
          });
          card.append(reject);
        } else if (item.status === 'active' || item.status === 'failed') {
          const form = document.createElement('form'); const label = document.createElement('label');
          const confirm = document.createElement('input'); confirm.type = 'checkbox'; confirm.required = true;
          label.append(confirm, document.createTextNode(item.status === 'failed'
            ? ' Clean up any partial account created by the failed grant'
            : ' Revoke and terminate this account'));
          const button = document.createElement('button'); button.type = 'submit'; button.textContent = 'Revoke access'; form.append(label, button);
          form.addEventListener('submit', async event => {
            event.preventDefault(); if (!confirm.checked) return;
            await submitSshAction(button, item.id,
              `/api/admin/ssh-access/grants/${encodeURIComponent(item.id)}/revoke`,
              { expected_version: item.version, confirmed: true }, 'Revocation failed');
          });
          card.append(form);
        }
        sshAccess.append(card);
      }
    };
    refresh();
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

/// Apply the shared, Platform-owned HTML frame to service-owned admin pages.
/// Services keep rendering their `<main id="admin-content">`; the platform
/// contributes navigation without taking ownership of their page contents.
pub async fn shell_middleware(request: Request, next: Next) -> Response {
    let path = request.uri().path().to_owned();
    let is_admin_document = request.method() == axum::http::Method::GET
        && (path == "/admin" || path.starts_with("/admin/"))
        && !path.starts_with("/admin/assets/");
    let response = next.run(request).await;
    if !is_admin_document
        || !response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/html"))
    {
        return response;
    }

    let (mut parts, body) = response.into_parts();
    let bytes = match to_bytes(body, MAX_ADMIN_PAGE_BYTES).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return html_response(
                StatusCode::BAD_GATEWAY,
                "text/html; charset=utf-8",
                "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><title>Admin page unavailable</title><body><main><h1>Admin page unavailable</h1><p>The service page exceeded the Platform shell size limit.</p></main></body></html>".to_owned(),
            );
        }
    };
    let Ok(html) = String::from_utf8(bytes.to_vec()) else {
        return Response::from_parts(parts, Body::from(bytes));
    };
    let Some(wrapped) = wrap_admin_document(&html, &path) else {
        return Response::from_parts(parts, Body::from(bytes));
    };

    parts.headers.remove(CONTENT_LENGTH);
    parts
        .headers
        .insert(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    Response::from_parts(parts, Body::from(wrapped))
}

fn wrap_admin_document(html: &str, current_path: &str) -> Option<String> {
    let lowered = html.to_ascii_lowercase();
    let body_start = lowered.find("<body")?;
    let body_open_end = lowered[body_start..].find('>')? + body_start + 1;
    let body_end = lowered.rfind("</body>")?;
    if !lowered.contains("<main id=\"admin-content\"") {
        return None;
    }
    if body_end < body_open_end
        || lowered[body_open_end..body_end].contains("<div class=\"shell\">")
    {
        return None;
    }

    let mut wrapped = String::with_capacity(html.len() + 1200);
    wrapped.push_str(&html[..body_open_end]);
    wrapped.push_str("<a class=\"skip-link\" href=\"#admin-content\">Skip to content</a>");
    wrapped.push_str("<div class=\"shell\">");
    wrapped.push_str(&render_admin_navigation(current_path));
    wrapped.push_str("<div class=\"admin-workspace\">");
    wrapped.push_str(&render_admin_header(current_path));
    wrapped.push_str(&html[body_open_end..body_end]);
    wrapped.push_str("</div></div>");
    wrapped.push_str(&html[body_end..]);
    Some(wrapped)
}

fn render_admin_header(current_path: &str) -> String {
    let normalized_path = if current_path == "/admin/" {
        "/admin"
    } else {
        current_path
    };
    let current = MODULES
        .iter()
        .filter(|section| {
            section
                .path
                .is_some_and(|path| section_path_matches(path, normalized_path))
        })
        .max_by_key(|section| section.path.map_or(0, str::len));
    let section_name = current.map_or("Administration", |section| section.name);
    format!(
        "<header class=\"admin-header\"><div><p class=\"eyebrow\">Current section</p><p class=\"admin-section-name\">{}</p></div><div class=\"admin-statuses\"><p class=\"admin-status-item\">Operator session <span class=\"status-pill status-protected\" data-operator-session role=\"status\" aria-live=\"polite\">Checking</span></p><p class=\"admin-status-item\">Platform <span class=\"status-pill\" data-platform-status role=\"status\" aria-live=\"polite\">Checking</span></p></div></header>",
        escape_html(section_name)
    )
}

fn render_admin_navigation(current_path: &str) -> String {
    let mut html = String::from(
        "<nav aria-label=\"Platform administration\"><p class=\"brand\">Vapor Platform</p>",
    );
    let mut current_group = "";
    for section in MODULES {
        if section.group != current_group {
            current_group = section.group;
            html.push_str(&format!(
                "<p class=\"nav-group\">{}</p>",
                escape_html(section.group)
            ));
        }
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
    let html = "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Vapor Admin</title><link rel=\"stylesheet\" href=\"/admin/assets/admin.css\"><script defer src=\"/admin/assets/admin.js\"></script></head><body><main id=\"admin-content\"><p class=\"eyebrow\">Platform</p><h1>Service overview</h1><section id=\"platform-overview\" aria-live=\"polite\"><p class=\"muted\">Loading protected service status…</p></section><section class=\"card\"><div class=\"request-queue-header\"><h2>Requests needing attention</h2><button id=\"operator-queue-refresh\" type=\"button\">Refresh</button></div><p class=\"muted\">Counts come from each owner service. Open the approvals inbox to review unanswered requests.</p><div id=\"operator-queue\" aria-live=\"polite\"><p class=\"muted\">Loading request queues…</p></div></section><section class=\"card operator-actions\"><h2>Operator work areas</h2><p class=\"muted\">Choose a workflow. Request counts and approval decisions remain with their owning services.</p><div class=\"action-links\"><a href=\"/admin/approvals\">Review pending approvals</a><a href=\"/admin/deployments\">Inspect deployments</a><a href=\"/admin/hosting\">Inspect service health</a><a href=\"/admin/ssh-access\">Manage temporary SSH access</a><a href=\"/admin/history\">Review request history</a></div></section></main></body></html>";
    html_response(StatusCode::OK, "text/html; charset=utf-8", html.to_owned())
}

async fn operations(
    headers: axum::http::HeaderMap,
    Query(query): Query<OperationsQuery>,
) -> Response {
    let Some(view) = OperationsView::parse(query.view.as_deref()) else {
        return html_response(
            StatusCode::BAD_REQUEST,
            "text/html; charset=utf-8",
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
        Err(_) => return auth_unavailable_response(crate::admin_auth::AuthFailure::Unavailable),
    };
    if let Err(failure) = crate::admin_auth::authorize_root_with_client(
        &headers,
        service_token.as_deref(),
        &endpoint,
        &client,
    )
    .await
    {
        return auth_unavailable_response(failure);
    }
    html_response(
        StatusCode::OK,
        "text/html; charset=utf-8",
        operations_page(view),
    )
}

fn operations_page(view: OperationsView) -> String {
    let content = match view {
        OperationsView::Approvals => "<section class=\"card\"><div class=\"request-queue-header\"><div><h2>Pending decisions</h2><p class=\"muted\">Review unanswered requests. Decisions are saved by Platform or Identity, the service that owns each request.</p></div><button id=\"approvals-refresh\" type=\"button\">Refresh</button></div><p id=\"approvals-feedback\" role=\"status\" aria-live=\"polite\"></p></section><section class=\"card\"><h2>Deployments</h2><div id=\"pending-deploy-approvals\" aria-live=\"polite\"><p class=\"muted\">Loading deployment approvals…</p></div></section><section class=\"card\"><h2>SSH access</h2><div id=\"pending-ssh-approvals\" aria-live=\"polite\"><p class=\"muted\">Loading SSH access requests…</p></div></section><section class=\"card\"><h2>Identity permissions</h2><div id=\"pending-identity-approvals\" aria-live=\"polite\"><p class=\"muted\">Loading Identity permission requests…</p></div></section>".to_owned(),
        OperationsView::Deployments => concat!(
            "<section class=\"card\"><h2>Deployment requests</h2><p class=\"muted\">Review the exact tested revision, CI result, and pinned service revisions. Approving records your decision and attempts to queue that deployment; the request card reports both stages.</p><div id=\"deploy-requests\" aria-live=\"polite\"><p class=\"muted\">Loading deployment requests…</p></div></section>",
            "<section class=\"card\"><h2>Latest deploy or restore result</h2><p class=\"muted\">This is the current Platform status pointer. Historical requests are listed separately under History.</p><div id=\"latest-operation-status\" aria-live=\"polite\"><p class=\"muted\">Loading latest operation result…</p></div><a href=\"/docs\">Open server runbooks</a></section>"
        )
        .to_owned(),
        OperationsView::Hosting => "<section class=\"card\"><h2>Service health and feature readiness</h2><p class=\"muted\">Health reports whether a service answers its probe. Feature readiness is shown only when the service reports it; “not reported” is not treated as ready.</p><div id=\"operations-status\" aria-live=\"polite\"><p class=\"muted\">Loading service status…</p></div></section><section class=\"card\"><h2>Runbooks</h2><p class=\"muted\">Use the service-owned recovery guidance for operational changes.</p><a href=\"/docs\">Open server runbooks</a></section>".to_owned(),
        OperationsView::TemporaryAccess => "<section class=\"card\"><h2>Temporary SSH access</h2><p class=\"muted\">Requests start with the requester and their key. Review the requested scope, then set the actual grant within server policy. Each grant uses a separate key-only account, expires automatically, and can be revoked immediately.</p><div id=\"ssh-access\" aria-live=\"polite\"><p class=\"muted\">Loading access requests and active grants…</p></div></section>".to_owned(),
        OperationsView::History => "<section class=\"card\"><h2>Request and operation history</h2><p class=\"muted\">Read-only Platform deploy/restore outcomes and owner-sourced deployment, access, and Identity request records. Each owning service remains authoritative for its records and decisions; an unavailable source is shown separately.</p><noscript><p>Enable JavaScript for the combined searchable view, or open each owner section directly: <a href=\"/admin/operations?view=deployments\">Deployments</a>, <a href=\"/admin/operations?view=temporary-access\">Temporary access</a>, and <a href=\"/admin/identity\">Identity requests</a>.</p></noscript><div class=\"history-toolbar\"><p id=\"history-count\" class=\"history-count muted\"></p><div><button id=\"history-load-more\" type=\"button\" hidden>Load older records</button> <button id=\"history-refresh\" type=\"button\">Refresh records</button></div></div><div class=\"history-controls\"><label for=\"history-search\">Search<input id=\"history-search\" type=\"search\" autocomplete=\"off\" placeholder=\"Repository, key fingerprint, account, request…\"></label><label for=\"history-kind\">Area<select id=\"history-kind\"><option value=\"all\">All areas</option><option>Platform operation</option><option>Deployment</option><option>Temporary access</option><option>Identity request</option></select></label><label for=\"history-status\">Status<select id=\"history-status\"><option value=\"all\">All statuses</option><option>pending</option><option>approved</option><option>executing</option><option>succeeded</option><option>success</option><option>failed</option><option>rejected</option><option>expired</option><option value=\"expired-pending\">expired · owner transition pending</option><option>cancelled</option><option>stale</option><option>active</option><option>provisioning</option><option>revoked</option><option value=\"dispatch-failed\">dispatch failed</option><option value=\"dispatch-unknown\">dispatch result unknown</option><option value=\"dispatch-dispatching\">dispatch in progress</option><option value=\"dispatch-queued\">queued for deployment</option><option value=\"dispatch-claimed\">claimed by worker</option><option value=\"dispatch-completed\">worker completed</option></select></label></div><div id=\"history-sources\" aria-live=\"polite\"><p class=\"muted\">Loading history sources…</p></div><div id=\"operations-history\" aria-live=\"polite\"><p class=\"muted\">Loading history…</p></div></section>".to_owned(),
    };
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{} · Platform · Vapor Admin</title><link rel=\"stylesheet\" href=\"/admin/assets/admin.css\"><script defer src=\"/admin/assets/admin.js\"></script></head><body><main id=\"admin-content\"><p class=\"eyebrow\">Platform · Operations</p><h1>{}</h1>{content}</main></body></html>",
        view.name(),
        view.name(),
        content = content
    )
}

fn operations_error_page(title: &str, message: &str) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{} · Operations · Vapor Admin</title><link rel=\"stylesheet\" href=\"/admin/assets/admin.css\"><script defer src=\"/admin/assets/admin.js\"></script></head><body><main id=\"admin-content\"><p class=\"eyebrow\">Platform · Operations</p><h1>Operations</h1><p class=\"error\">{}</p><a href=\"/admin/approvals\">Return to Approvals</a></main></body></html>",
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
        assert!(html.contains("Decisions are saved by Platform or Identity"));
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
        assert!(script.contains("const isExpiredByTime = item => Number(item.expires_at_unix) > 0"));
        assert!(script.contains("pending.filter(isExpiredByTime)"));
        assert!(script.contains("pending.filter(item => !isExpiredByTime(item))"));
        assert!(script.contains("Expired by time · Identity has not yet recorded the transition"));
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
            get_response.headers().get("cache-control").unwrap(),
            "private, no-store"
        );

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

    #[test]
    fn current_operation_pointer_lives_with_deployments_not_history() {
        let deployments = super::operations_page(super::OperationsView::Deployments);
        let history = super::operations_page(super::OperationsView::History);
        assert!(deployments.contains("Latest deploy or restore result"));
        assert!(deployments.contains("id=\"latest-operation-status\""));
        assert!(!history.contains("latest-operation-status"));
        assert!(history.contains("Request and operation history"));
        assert!(history.contains("value=\"dispatch-claimed\">claimed by worker"));
        assert!(history.contains("value=\"dispatch-completed\">worker completed"));
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
    }

    #[test]
    fn operations_error_page_loads_shared_shell_status_script() {
        let page = super::operations_error_page("Unavailable", "Try again later.");
        assert!(page.contains("/admin/assets/admin.js"));
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
        assert!(script.contains("'Grant failed');"));
        assert!(script
            .contains("const submitSshAction = async (button, itemId, path, body, fallback) =>"));
        assert!(script.contains("No response was received; the request may have completed."));
        assert!(
            script.contains("/api/admin/ssh-access/requests/${encodeURIComponent(item.id)}/events")
        );
        assert!(script.contains("Access history"));
        assert!(script.contains("if (item.granted_duration_seconds != null)"));
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
        assert!(script.contains("Decision: ${item.status"));
        assert!(
            script.contains("Deployment: ${lifecycleLabel || dispatchLabels[item.dispatch_state]")
        );
        assert!(script.contains("expired: 'expired before worker claim'"));
        assert!(script.contains("claimed: 'worker claimed deployment'"));
        assert!(script.contains("completed: 'worker finished deployment'"));
        assert!(script.contains("worker stopped; deployment outcome unknown"));
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
        assert!(script.contains("? '; retry available' : '';"));
        assert!(script.contains("item.status === 'approved' && item.retry_eligible"));
        assert!(script.contains("const validDeploymentSelection = item =>"));
        assert!(script.contains("serviceDeployTargets.has(item.service_target)"));
        assert!(script.contains("Target: ${deploymentSelectionLabel(item)}"));
        assert!(script.contains(
            "const pendingActions = selectionValid ? ['approve', 'reject'] : ['reject']"
        ));
        assert!(
            script.contains("selectionValid && item.status === 'approved' && item.retry_eligible")
        );
        assert!(script.contains("const body = await result.clone().json()"));
        assert!(script.contains("actionFeedback.set(item.id, String(message).slice(0, 240))"));
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
