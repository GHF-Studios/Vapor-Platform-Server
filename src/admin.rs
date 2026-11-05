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
    service_id: Option<&'static str>,
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
        service_id: Some("platform"),
        state: SectionState::Implemented,
    },
    AdminSection {
        group: "Catalog",
        name: "Workshop / Registry",
        path: Some("/admin/workshop"),
        description: "Registry-owned catalog inventory",
        service_id: Some("registry"),
        state: SectionState::Implemented,
    },
    AdminSection {
        group: "Accounts",
        name: "Identity",
        path: Some("/admin/identity"),
        description: "Accounts and access administration",
        service_id: Some("identity"),
        state: SectionState::Implemented,
    },
    AdminSection {
        group: "Distribution",
        name: "Artifacts",
        path: Some("/admin/artifacts"),
        description: "Signed Vapor releases",
        service_id: Some("artifacts"),
        state: SectionState::Implemented,
    },
    AdminSection {
        name: "Build Cache",
        group: "Distribution",
        path: Some("/admin/build-cache"),
        description: "Per-target compiler snapshots",
        service_id: Some("build-cache"),
        state: SectionState::Implemented,
    },
    AdminSection {
        group: "Distribution",
        name: "Native builders",
        path: None,
        description: "VPS-managed Linux and Windows build orchestration is not available yet",
        service_id: None,
        state: SectionState::Planned,
    },
    AdminSection {
        group: "Operations",
        name: "Operations",
        path: Some("/admin/operations"),
        description: "Deployments, temporary access, and history",
        service_id: Some("platform"),
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
"#;

const JS: &str = r#"
(() => {
  const main = document.querySelector('main#admin-content');
  if (!main) return;
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
      for (const link of document.querySelectorAll('[data-admin-service]')) {
        const status = link.querySelector('[data-nav-status]');
        if (!status) continue;
        const service = services && services.find(item => item.id === link.dataset.adminService);
        if (!service || (service.health !== 'healthy' && service.health !== 'unavailable')) {
          status.textContent = 'Unknown';
          status.className = 'nav-status status-unknown';
          continue;
        }
        const healthy = service.health === 'healthy';
        status.textContent = healthy ? 'Healthy' : 'Offline';
        status.className = `nav-status status-${healthy ? 'healthy' : 'unavailable'}`;
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
    const platformHealthy = platformService && platformService.health === 'healthy';
    setPill(platform, platformHealthy ? 'Healthy' : 'Unavailable', platformHealthy ? 'healthy' : 'unavailable');
    updateNavHealth(services);
  };
  const overview = document.querySelector('#platform-overview, #operations-status');
  if (overview) {
    fetch('/api/admin/overview', { credentials: 'same-origin', cache: 'no-store' })
      .then(async response => ({ response, body: await response.json().catch(() => null) }))
      .then(({ response, body }) => {
        updateShellStatus(response, body);
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

  const history = document.querySelector('#operations-history');
  if (history) {
    fetch('/api/admin/operations', { credentials: 'same-origin', cache: 'no-store' })
      .then(async response => ({ response, body: await response.json().catch(() => null) }))
      .then(({ response, body }) => {
        history.replaceChildren();
        if (!response.ok) {
          history.textContent = response.status === 401 ? 'Sign in to view operation history.' :
            response.status === 403 ? 'Root access is required to view operation history.' :
            'Operation history is temporarily unavailable.';
          return;
        }
        if (!body || body.record_status !== 'reported' || !body.record) {
          history.textContent = body && body.record_status === 'invalid' ?
            'The latest operation record is invalid.' : 'No runtime operation record is reported.';
          return;
        }
        const record = body.record;
        const title = document.createElement('h3');
        title.textContent = `Latest ${record.operation}: ${record.result}`;
        const details = document.createElement('p');
        details.className = 'muted';
        details.textContent = `Revision ${record.revision} · ${record.server_mode} · started ${record.started_at_utc} · finished ${record.finished_at_utc}`;
        history.append(title, details);
      })
      .catch(() => {
        history.textContent = 'Operation history is temporarily unavailable.';
      });
  }

  const deployRequests = document.querySelector('#deploy-requests');
  if (deployRequests) {
    const actionFeedback = new Map();
    const refreshDeployRequests = async () => {
      let response;
      let requests;
      try {
        response = await fetch('/api/admin/deploy-requests', { credentials: 'same-origin', cache: 'no-store' });
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
      const items = Array.isArray(requests) ? requests : [];
      if (!items.length) {
        deployRequests.textContent = 'No deploy requests have been recorded.';
        return;
      }
      for (const item of items) {
        const card = document.createElement('article');
        card.className = 'card';
        const heading = document.createElement('h3');
        heading.textContent = `${item.repository} · ${item.branch} · ${item.status}`;
        const decisionStage = document.createElement('p');
        decisionStage.className = 'muted';
        decisionStage.textContent = `Decision: ${item.status === 'pending' ? 'awaiting review' : item.status === 'rejected' ? 'rejected' : item.status === 'expired' ? 'expired' : 'approved and saved'}`;
        const dispatchStage = document.createElement('p');
        dispatchStage.className = 'muted';
        const dispatchLabels = {
          not_attempted: 'not attempted', dispatching: 'dispatch in progress',
          queued: 'queued', expired: 'expired before worker claim',
          failed: 'dispatch failed', unknown: 'dispatch result unknown'
        };
        const retryLabel = item.retry_eligible && ['failed', 'unknown'].includes(item.dispatch_state)
          ? '; retry available' : '';
        dispatchStage.textContent = `Deployment: ${dispatchLabels[item.dispatch_state] || 'not reported'}${retryLabel}${item.dispatch_attempts ? ` · attempt ${item.dispatch_attempts}` : ''}${item.dispatch_detail ? ` · ${String(item.dispatch_detail).slice(0, 240)}` : ''}`;
        const revision = document.createElement('p');
        revision.textContent = `Commit ${item.commit} · tree ${item.tree}`;
        const pins = document.createElement('p');
        pins.className = 'muted';
        pins.textContent = `${Array.isArray(item.submodules) ? item.submodules.length : 0} pinned service revisions · expires ${item.expires_at_unix}`;
        const summary = document.createElement('p');
        summary.textContent = item.summary;
        const workflow = document.createElement('a');
        workflow.href = item.workflow_url;
        workflow.rel = 'noopener noreferrer';
        workflow.textContent = 'Open successful CI run';
        card.append(heading, decisionStage, dispatchStage, revision, pins, summary, workflow);
        if (actionFeedback.has(item.id)) {
          const feedback = document.createElement('p');
          feedback.className = 'muted';
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
        for (const decision of (item.status === 'pending' ? ['approve', 'reject'] : item.status === 'approved' && item.retry_eligible ? ['dispatch'] : item.status === 'executing' ? ['reconcile'] : [])) {
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
              const message = body?.error?.message || plainText.trim().slice(0, 240) || (result.ok ? 'Action completed.' : `Action failed (${result.status}).`);
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
        response = await fetch('/api/admin/ssh-access/requests', { credentials: 'same-origin', cache: 'no-store' });
      } catch (_) { sshAccess.textContent = 'Could not contact the temporary SSH service.'; return; }
      sshAccess.replaceChildren();
      if (!response.ok) {
        await showResponseFailure(sshAccess, response, 'Could not load temporary SSH requests');
        return;
      }
      items = await response.json().catch(() => null);
      if (!Array.isArray(items)) { sshAccess.textContent = 'The temporary SSH service returned an invalid response.'; return; }
      if (!items.length) { sshAccess.textContent = 'No temporary SSH requests.'; return; }
      for (const item of items) {
        const card = document.createElement('article'); card.className = 'card';
        const title = document.createElement('h3'); title.textContent = `${item.status} · ${item.fingerprint}`;
        const purpose = document.createElement('p'); purpose.textContent = item.purpose;
        const scope = document.createElement('p'); scope.className = 'muted';
        scope.textContent = `Requested: ${item.requested_capabilities.join(', ') || 'shell only'} · ${item.requested_duration_seconds}s`;
        card.append(title, purpose, scope);
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
        .route("/api/admin/overview", get(overview))
        .route("/api/admin/operations", get(operations_status))
        .route("/admin/assets/admin.css", get(stylesheet))
        .route("/admin/assets/admin.js", get(script))
        .merge(crate::deploy_requests::router())
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
                let service = section.service_id.unwrap_or("");
                html.push_str(&format!(
                    "<a href=\"{}\" title=\"{}\" data-admin-service=\"{}\"{}><span>{}</span><small class=\"nav-status status-unknown\" data-nav-status>Checking</small></a>",
                    escape_html(path),
                    escape_html(section.description),
                    escape_html(service),
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
    let html = "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Vapor Admin</title><link rel=\"stylesheet\" href=\"/admin/assets/admin.css\"><script defer src=\"/admin/assets/admin.js\"></script></head><body><main id=\"admin-content\"><p class=\"eyebrow\">Platform</p><h1>Service overview</h1><section id=\"platform-overview\" aria-live=\"polite\"><p class=\"muted\">Loading protected service status…</p></section><section class=\"card operator-actions\"><h2>Operator work areas</h2><p class=\"muted\">Choose a workflow. The overview does not report pending request counts or infer feature readiness.</p><div class=\"action-links\"><a href=\"/admin/operations?view=deployments\">Review deployment requests</a><a href=\"/admin/operations?view=hosting\">Inspect service health</a><a href=\"/admin/operations?view=temporary-access\">Review temporary access</a><a href=\"/admin/operations?view=history\">View latest operation result</a></div></section></main></body></html>";
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
                "Choose Deployments, Hosting, Temporary Access, or History.",
            ),
        );
    };
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
    let tabs = [
        OperationsView::Deployments,
        OperationsView::Hosting,
        OperationsView::TemporaryAccess,
        OperationsView::History,
    ]
    .into_iter()
    .map(|tab| {
        let current = if std::mem::discriminant(&view) == std::mem::discriminant(&tab) {
            " aria-current=\"page\""
        } else {
            ""
        };
        format!(
            "<a href=\"/admin/operations?view={}\"{}>{}</a>",
            tab.query_value(),
            current,
            tab.name()
        )
    })
    .collect::<String>();

    let content = match view {
        OperationsView::Deployments => "<section class=\"card\"><h2>Deployment requests</h2><p class=\"muted\">Review the exact tested revision, CI result, and pinned service revisions. Approving records your decision and attempts to queue that deployment; the request card reports both stages.</p><div id=\"deploy-requests\" aria-live=\"polite\"><p class=\"muted\">Loading deployment requests…</p></div></section>".to_owned(),
        OperationsView::Hosting => "<section class=\"card\"><h2>Service health and feature readiness</h2><p class=\"muted\">Health reports whether a service answers its probe. Feature readiness is shown only when the service reports it; “not reported” is not treated as ready.</p><div id=\"operations-status\" aria-live=\"polite\"><p class=\"muted\">Loading service status…</p></div></section><section class=\"card\"><h2>Runbooks</h2><p class=\"muted\">Use the service-owned recovery guidance for operational changes.</p><a href=\"/docs\">Open server runbooks</a></section>".to_owned(),
        OperationsView::TemporaryAccess => "<section class=\"card\"><h2>Temporary SSH access</h2><p class=\"muted\">Requests start with the requester and their key. Review the requested scope, then set the actual grant within server policy. Each grant uses a separate key-only account, expires automatically, and can be revoked immediately.</p><div id=\"ssh-access\" aria-live=\"polite\"><p class=\"muted\">Loading access requests and active grants…</p></div></section>".to_owned(),
        OperationsView::History => "<section class=\"card\"><h2>Latest operation result</h2><p class=\"muted\">The current Platform API exposes a latest-status pointer, not a complete event history. This view reports only that record.</p><div id=\"operations-history\" aria-live=\"polite\"><p class=\"muted\">Loading latest operation result…</p></div><a href=\"/docs\">Open server runbooks</a></section>".to_owned(),
    };
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{} · Operations · Vapor Admin</title><link rel=\"stylesheet\" href=\"/admin/assets/admin.css\"><script defer src=\"/admin/assets/admin.js\"></script></head><body><main id=\"admin-content\"><p class=\"eyebrow\">Platform · Operations</p><h1>Operations</h1><nav class=\"operation-tabs\" aria-label=\"Operations views\">{tabs}</nav><section aria-labelledby=\"operations-view-title\"><h2 id=\"operations-view-title\">{}</h2>{content}</section></main></body></html>",
        view.name(),
        view.name(),
        content = content,
        tabs = tabs
    )
}

fn operations_error_page(title: &str, message: &str) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{} · Operations · Vapor Admin</title><link rel=\"stylesheet\" href=\"/admin/assets/admin.css\"></head><body><main id=\"admin-content\"><p class=\"eyebrow\">Platform · Operations</p><h1>Operations</h1><p class=\"error\">{}</p><a href=\"/admin/operations\">Return to Deployments</a></main></body></html>",
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
    Deployments,
    Hosting,
    TemporaryAccess,
    History,
}

impl OperationsView {
    fn parse(value: Option<&str>) -> Option<Self> {
        match value.unwrap_or("deployments") {
            "deployments" => Some(Self::Deployments),
            "hosting" => Some(Self::Hosting),
            "temporary-access" => Some(Self::TemporaryAccess),
            "history" => Some(Self::History),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Deployments => "Deployments",
            Self::Hosting => "Hosting",
            Self::TemporaryAccess => "Temporary Access",
            Self::History => "History",
        }
    }

    fn query_value(self) -> &'static str {
        match self {
            Self::Deployments => "deployments",
            Self::Hosting => "hosting",
            Self::TemporaryAccess => "temporary-access",
            Self::History => "history",
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
    let (platform, docs, identity, diagnostics, registry, artifacts, build_cache) = tokio::join!(
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
        assert!(html.contains("href=\"/admin/operations\""));
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
        assert!(script.contains("Deployment: ${dispatchLabels[item.dispatch_state]"));
        assert!(script.contains("expired: 'expired before worker claim'"));
        assert!(script.contains("? '; retry available' : '';"));
        assert!(script.contains("item.status === 'approved' && item.retry_eligible"));
        assert!(script.contains("const body = await result.clone().json()"));
        assert!(script.contains("actionFeedback.set(item.id, String(message).slice(0, 240))"));
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
        assert_eq!(health_requests.load(Ordering::SeqCst), 7);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let overview: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(overview["state"], "serving");
        assert_eq!(overview["services"][0]["id"], "platform");
        assert_eq!(overview["services"][0]["health"], "healthy");
        assert_eq!(overview["services"][1]["health"], "unavailable");
        assert_eq!(overview["services"][6]["health"], "healthy");
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
