use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{HeaderValue, StatusCode};
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

const MODULES: &[(&str, &str, &str, bool)] = &[
    ("Platform", "/admin", "Read-only host overview", true),
    (
        "Operations",
        "/admin/operations",
        "Live status and runbooks",
        true,
    ),
    (
        "Workshop",
        "/admin/workshop",
        "Registry-owned catalog",
        true,
    ),
    ("Identity", "/admin/identity", "Accounts and access", true),
    ("Artifacts", "/admin/artifacts", "Vapor releases", true),
    (
        "Build Cache",
        "/admin/build-cache",
        "Target snapshots",
        true,
    ),
];

const CSS: &str = r#"
:root { color-scheme: dark; font: 14px/1.45 system-ui, sans-serif; background:#111821; color:#e4eaf1; }
* { box-sizing:border-box; }
body { margin:0; min-height:100vh; }
.shell { min-height:100vh; display:grid; grid-template-columns:220px 1fr; }
nav { padding:22px 14px; background:#182330; border-right:1px solid #2b3a4b; }
.brand { margin:0 0 18px; font-size:16px; letter-spacing:.03em; }
nav a { display:block; padding:9px 10px; margin:3px 0; border-radius:5px; color:#c7d2de; text-decoration:none; }
nav a:hover, nav a[aria-current="page"] { background:#26394e; color:#fff; }
.nav-disabled { display:flex; justify-content:space-between; gap:8px; padding:9px 10px; margin:3px 0; color:#788797; }
.nav-status { color:#b7a564; font-size:11px; white-space:nowrap; }
.service-grid { display:grid; grid-template-columns:repeat(auto-fit,minmax(220px,1fr)); gap:10px; }
.service-card { background:#182330; border:1px solid #2b3a4b; border-radius:6px; padding:12px; }
.service-card h2 { font-size:15px; margin:0 0 8px; }
.service-state { font-weight:650; }
.state-healthy { color:#6fc58a; }
.state-unavailable { color:#ed7a80; }
.state-planned { color:#d4bd69; }
main { padding:30px clamp(20px,4vw,56px); max-width:1100px; }
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
@media(max-width:720px) { .shell { grid-template-columns:1fr; } nav { border-right:0; border-bottom:1px solid #2b3a4b; padding:12px; } nav a { display:inline-block; } main { padding:20px 14px; } }
"#;

const JS: &str = r#"
(() => {
  const main = document.querySelector('main#admin-content');
  if (!main) return;
  const modules = [
    ['Platform', '/admin', true],
    ['Operations', '/admin/operations', true],
    ['Workshop', '/admin/workshop', true],
    ['Identity', '/admin/identity', true],
    ['Artifacts', '/admin/artifacts', true],
    ['Build Cache', '/admin/build-cache', true],
  ];
  const shell = document.createElement('div');
  shell.className = 'shell';
  const nav = document.createElement('nav');
  nav.setAttribute('aria-label', 'Admin modules');
  const brand = document.createElement('p');
  brand.className = 'brand';
  brand.textContent = 'Vapor Platform';
  nav.append(brand);
  for (const [label, href, available] of modules) {
    if (available) {
      const link = document.createElement('a');
      link.href = href;
      link.textContent = label;
      if (location.pathname === href) link.setAttribute('aria-current', 'page');
      nav.append(link);
    } else {
      const item = document.createElement('span');
      item.className = 'nav-disabled';
      item.setAttribute('aria-disabled', 'true');
      const name = document.createElement('span');
      name.textContent = label;
      const status = document.createElement('small');
      status.className = 'nav-status';
      status.textContent = 'Planned';
      item.append(name, status);
      nav.append(item);
    }
  }
  main.parentNode.insertBefore(shell, main);
  shell.append(nav, main);

  const overview = document.querySelector('#platform-overview, #operations-status');
  if (overview) {
    fetch('/api/admin/overview', { credentials: 'same-origin', cache: 'no-store' })
      .then(async response => ({ response, body: await response.json().catch(() => null) }))
      .then(({ response, body }) => {
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
        overview.textContent = 'Service status is temporarily unavailable.';
      });
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
        card.append(heading, revision, pins, summary, workflow);
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
        for (const decision of (item.status === 'pending' ? ['approve', 'reject'] : item.status === 'approved' ? ['dispatch'] : item.status === 'executing' ? ['reconcile'] : [])) {
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
            const result = await fetch(`/api/admin/deploy-requests/${encodeURIComponent(item.id)}/${decision}`, {
              method: 'POST', credentials: 'same-origin', cache: 'no-store',
              headers: { 'content-type': 'application/json' },
              body: JSON.stringify(['dispatch', 'reconcile'].includes(decision)
                ? { expected_version: item.version, confirmed: true }
                : { expected_version: item.version, confirmed: true,
                    reason: decision === 'approve' ? 'approved in Platform Operations' : 'rejected in Platform Operations' })
            });
            if (result.ok) await refreshDeployRequests();
            else { button.disabled = false; button.textContent = `Failed (${result.status}); reload`; }
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
    const refresh = async () => {
      let response;
      let items;
      try {
        response = await fetch('/api/admin/ssh-access/requests', { credentials: 'same-origin', cache: 'no-store' });
        items = await response.json().catch(() => null);
      } catch (_) { sshAccess.textContent = 'Temporary SSH requests are unavailable.'; return; }
      sshAccess.replaceChildren();
      if (!response.ok) { sshAccess.textContent = 'Root access is required to review SSH requests.'; return; }
      if (!Array.isArray(items) || !items.length) { sshAccess.textContent = 'No temporary SSH requests.'; return; }
      for (const item of items) {
        const card = document.createElement('article'); card.className = 'card';
        const title = document.createElement('h3'); title.textContent = `${item.status} · ${item.fingerprint}`;
        const purpose = document.createElement('p'); purpose.textContent = item.purpose;
        const scope = document.createElement('p'); scope.className = 'muted';
        scope.textContent = `Requested: ${item.requested_capabilities.join(', ') || 'shell only'} · ${item.requested_duration_seconds}s`;
        card.append(title, purpose, scope);
        if (item.detail) {
          const detail = document.createElement('p'); detail.className = 'muted'; detail.textContent = item.detail;
          card.append(detail);
        }
        if (item.expires_at_unix) {
          const expiry = document.createElement('p'); expiry.className = 'muted';
          expiry.textContent = `${item.status === 'active' ? 'Expires' : 'Approved expiry'}: ${new Date(item.expires_at_unix * 1000).toLocaleString()}`;
          card.append(expiry);
        }
        if (item.status === 'active' && item.username) {
          const login = document.createElement('p');
          login.textContent = `SSH account: ${item.username}@${item.host} · port 22 · key ${item.fingerprint}`;
          card.append(login);
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
            button.disabled = true;
            const result = await fetch(`/api/admin/ssh-access/requests/${encodeURIComponent(item.id)}/approve`, {
              method: 'POST', credentials: 'same-origin', cache: 'no-store', headers: { 'content-type': 'application/json' },
              body: JSON.stringify({ expected_version: item.version, confirmed: true, host_admin_confirmed: adminConfirm.checked,
                granted_capabilities: selected, granted_duration_seconds: Number(duration.value) })
            });
            if (result.ok) await refresh(); else { button.disabled = false; button.textContent = `Failed (${result.status}); reload`; }
          });
          card.append(form);
          const reject = document.createElement('button'); reject.type = 'button'; reject.textContent = 'Reject request';
          reject.addEventListener('click', async () => {
            reject.disabled = true;
            const result = await fetch(`/api/admin/ssh-access/requests/${encodeURIComponent(item.id)}/reject`, {
              method: 'POST', credentials: 'same-origin', cache: 'no-store', headers: { 'content-type': 'application/json' },
              body: JSON.stringify({ expected_version: item.version, confirmed: true })
            });
            if (result.ok) await refresh(); else { reject.disabled = false; reject.textContent = `Failed (${result.status}); reload`; }
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
            event.preventDefault(); if (!confirm.checked) return; button.disabled = true;
            const result = await fetch(`/api/admin/ssh-access/grants/${encodeURIComponent(item.id)}/revoke`, {
              method: 'POST', credentials: 'same-origin', cache: 'no-store', headers: { 'content-type': 'application/json' },
              body: JSON.stringify({ expected_version: item.version, confirmed: true })
            });
            if (result.ok) await refresh(); else { button.disabled = false; button.textContent = `Failed (${result.status}); reload`; }
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

async fn landing() -> Response {
    let links = MODULES
        .iter()
        .map(|(name, path, description, available)| {
            if *available {
                format!(
                    "<a href=\"{}\" title=\"{}\">{}</a>",
                    escape_html(path),
                    escape_html(description),
                    escape_html(name)
                )
            } else {
                format!(
                    "<span class=\"nav-disabled\" aria-disabled=\"true\"><span title=\"{}\">{}</span><small class=\"nav-status\">Planned</small></span>",
                    escape_html(description),
                    escape_html(name)
                )
            }
        })
        .collect::<String>();
    let html = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Vapor Admin</title><link rel=\"stylesheet\" href=\"/admin/assets/admin.css\"><script defer src=\"/admin/assets/admin.js\"></script></head><body><div class=\"shell\"><nav aria-label=\"Admin modules\"><p class=\"brand\">Vapor Platform</p>{links}</nav><main><p class=\"eyebrow\">Platform</p><h1>Service overview</h1><section id=\"platform-overview\" aria-live=\"polite\"><p class=\"muted\">Loading protected service status…</p></section></main></div></body></html>"
    );
    html_response(StatusCode::OK, "text/html; charset=utf-8", html)
}

async fn operations(headers: axum::http::HeaderMap) -> Response {
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
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Operations · Vapor Admin</title><link rel=\"stylesheet\" href=\"/admin/assets/admin.css\"><script defer src=\"/admin/assets/admin.js\"></script></head><body><main id=\"admin-content\"><p class=\"eyebrow\">Platform · Operations</p><h1>Operations</h1><p class=\"muted\">Review the exact tested revision before approving a production deploy.</p><section id=\"operations-status\" aria-live=\"polite\"><p class=\"muted\">Loading protected service status…</p></section><section class=\"card\"><h2>Pending deploy approvals</h2><div id=\"deploy-requests\" aria-live=\"polite\"><p class=\"muted\">Loading pending requests…</p></div></section><section class=\"card\"><h2>Temporary SSH access</h2><p class=\"muted\">Requests begin with the requester and their key. Set the actual grant within server policy; each grant uses a separate key-only account, expires automatically, and can be revoked immediately.</p><div id=\"ssh-access\" aria-live=\"polite\">Loading SSH requests…</div></section><section class=\"card\"><h2>Deployment and recovery history</h2><div id=\"operations-history\" aria-live=\"polite\"><p class=\"muted\">Loading protected operation history…</p></div><a href=\"/docs\">Open server runbooks and recovery docs</a></section></main></body></html>".to_owned(),
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
            json!({"allowed": valid, "expires_at_unix": 0, "profile_id": if valid { Some("mock-root-profile") } else { None }}),
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
        let response = super::router()
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
        let response = super::router()
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
        let response = super::router()
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
        let response = super::router()
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
        let response = super::router()
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
        let get_response = super::router()
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

        let post_response = super::router()
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
    async fn operations_history_api_requires_root_before_reading_record() {
        let response = super::router()
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
