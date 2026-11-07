# Vapor server deployment status

## Public web surface spot-check (2026-11-07 17:06 UTC)

- Public `/`, `/downloads`, and `/healthz` returned `200`; `/healthz` returned
  `ok`. The homepage now serves the product overview and links to Downloads,
  Docs, License, and Impressum. Downloads explains that Mad Max and Loo Cast
  files are acquired separately and locally, and that the current Artifact
  bundle is an application archive rather than an installer; guided install
  and one-click updates are still in progress.
- Unauthenticated `/admin/approvals` returned `401`, as expected for a
  protected owner page. This spot-check did not re-check the running Platform
  revision or service topology; the full host snapshot below remains dated
  15:13 UTC.
- `/api/build-cache/v1/status` still reports no configured/current snapshot
  for either supported target (`x86_64-unknown-linux-gnu` and
  `x86_64-pc-windows-gnullvm`). The deployed status API therefore does not
  show a promoted remote cache artifact; deployment builds continue to use the
  primary VPS fallback path recorded in the host snapshot below.

## Primary VPS worker-capacity spot-check (2026-11-07 17:09 UTC)

- The live host reports one vCPU, 1.8 GiB RAM (1.3 GiB available), and 40 GiB
  free on `/`. It exposes AMD SVM under KVM, and `/dev/kvm` is `root:kvm`
  mode `0660`; the `leslieghf` operator account is not in `kvm`.
- Engineering inference: this is enough to retain the current local-build
  fallback, but there is no useful spare CPU/RAM for a concurrent VM compiler
  worker. If virtualization-based worker builds remain the plan, put them on
  the optional second node after its capacity is selected. No VM or host
  configuration was changed.

## Current live state (2026-11-07 15:13 UTC)

This is the newest verified state; older entries below are historical snapshots
and must not be read as current host status.

- The VPS is running Platform commit
  `12192d85348432ee24d1ce7c02ff2dab6eeded84` in `services` mode. The independent
  Homepage, Docs, Identity, Diagnostics, Registry, Artifact, and Build Cache
  services, the Platform gateway, and Caddy are active. The deployed
  `deploy/scripts/health-check.sh` completed successfully over root SSH; it
  checks each service endpoint, Caddy's implemented admin routes, unauthenticated
  service contracts, Atlas through Caddy, and the enabled/active state-export
  timer. The retired deploy polling timer remains inactive.
- Caddy routes `/cli/ssh/*` to Identity. The SSH authorization API returns
  `422` for an empty request body and the authorization page returns `400`
  without its required query string; these expected validation responses
  confirm both requests reach Identity. The unauthenticated Operations page
  returns `401` as expected.
- Public `/` and `/downloads` return `200`. `/downloads` contains the current
  standalone setup and local-game-source guidance. Both signed stable release
  endpoints (`windows-x86_64` and `linux-x86_64`) currently return `404`, so
  no installer/archive is advertised as downloadable. At this live check, the
  root page was still a minimal link page; the detailed first-run journey is at
  `/downloads`.
- Public Build Cache status returns `200` and lists both supported target
  triples, but reports `configured=false` and `current=false` for each. There
  is no promoted Linux or Windows target snapshot and no worker is producing
  them; approved Platform service deploys still build locally on the primary
  VPS, as required by the fallback path.
- The successful bootstrap created a root-only pre-cutover archive at
  `/var/backups/vapor-server/vapor-platform-recovery-20261107T143242Z-6f6756d3ec6b.tar.gz`.
  The exporter subsequently published recovery generation
  `/var/backups/vapor-server/vapor-platform-recovery-20261107T151352Z-12192d853484.tar.gz`
  (4,808,276 bytes; SHA-256
  `6c9973d1d5e8d9bb5880df3f4a63a4a81cdc920975fd49e4b014fcb0dc9d0b2b`). Its
  sidecar, archive checksum, bundle structure, and manifest all validate; the
  latest pointer agrees with the index. The retention planner confirms the
  current generation is trusted, with 19 verified indexed archives and a
  168-generation retention limit. No archives were pruned. The exporter's
  `existing latest/index metadata is not consistent` warning was a false
  positive: the CLI emitted it whenever the safe prune list was empty, even
  when metadata was valid and the retention limit had not been reached. A
  source fix now reports that warning only when metadata is actually missing
  or inconsistent. Commit `c7b6a33` passed both `root-ci` and
  `windows-recovery-tests` in GitHub Actions run `42385701950`; the fix has not
  yet been deployed. The periodic deploy timer was not started.
- The three pending full-deploy requests are `f08c9dec-627f-414e-b4e5-54562100df2f`
  (`7d07590`), `ed74751d-d3b2-4e67-a719-744121bd34d5` (`96e650f`), and
  `3196a6b8-2eb2-4957-b56a-2b7241bfce7d` (`12192d8`). The first two are
  superseded by the live `12192d8` deployment; the latest request was created
  automatically after the direct, owner-authorized recovery deploy. No request
  was approved by that direct deployment. These records were left unchanged;
  the dashboard should reject obsolete pending requests and reconcile the
  latest request with the already-live revision through an audited operation.
- No 128 GB removable recovery drive is currently mounted on the Windows
  workstation (the only FAT32 volume is about 1 GB). The real USB pull and
  clean-host restore drill remain unverified; no Task Scheduler job was
  registered against an assumed drive letter.
- The reusable SSH bootstrap branch has been updated to pin `12192d8` and to
  check the `/cli/ssh/authorize` route as well as the API route. It is a future
  recovery artifact; the current VPS is already cut over.

## Not-yet-deployed public service UI updates (2026-11-07 15:32 UTC)

- Homepage commit `589be8b5b8438337334fdf5200b07292899e3270` replaces the
  link-only root page with a responsive product overview and directs setup to
  the existing `/downloads` page. It keeps release status, setup details, and
  legal routes owned by their current surfaces. `cargo check --locked` passed
  for the Homepage crate; this commit is not yet in the live Platform
  composition or deployed on the VPS.
- Build Cache commit `cb0a281bece554b709b4d702a342aa0d39b818c9` corrects its
  admin page: the service validates and promotes candidate archives submitted
  by Platform, but does not run builds. This matches the current implementation
  and the snapshot contract; it has not yet been included in the live
  composition or deployed.

## Source candidate and access recheck (2026-11-07 15:44 UTC)

- Platform candidate `50729bc3b141663fba2f3f8005f631a35f9c7085` tightens the
  Deployments cards: the default view keeps repository/branch, status, summary,
  target, and current stage visible; requester, pinned revisions, CI evidence,
  dispatch detail, and result detail are expandable. Retry messaging now uses
  the same target/status/dispatch eligibility as the retry action. The result
  detail renderer no longer references a block-scoped element outside its
  creation scope.
- The local Rust workspace check and embedded JavaScript syntax check passed.
  GitHub Actions run `42387570348` passed both `root-ci` and
  `windows-recovery-tests`. This is source/CI evidence only; no live deployment
  was attempted.
- A direct SSH attempt with the workstation's designated root key
  (`SHA256:rXxD/039aN25aOaqJIFBFveVlgTWd8grNcdJ9ofIg5c`) was rejected with
  `Permission denied (publickey)`. No alternate credentials were tried and no
  VPS files or services were changed. The last verified live revision remains
  the one recorded above until a new host check succeeds.

## Main candidate approval and host-key recheck (2026-11-07 16:17 UTC)

- Platform `b1e2d3c874aca2cc9093d77db779238e907a063c` is on `main`; composed
  `root-ci`, Windows recovery tests, and the deploy-request submission all
  succeeded in Actions run `42388201962`. The submission job explicitly reports
  `Pending root approval request created.` This is a pending approval, not a
  deployment. The live root page still serves the old `<title>Vapor</title>`
  link page; `/downloads` is current, `/healthz` is healthy, and the protected
  approval/deployment/SSH pages correctly return `401` without an owner session.
- The same designated key was offered explicitly to both `root` and the
  temporary account `vpsssh_240aab1d`; the SSH server rejected both. Its local
  public fingerprint matches the owner-reported active grant fingerprint.
  The earlier root-shell `getent` check found that temporary account absent.
  Repeating a read-only login check at 16:17 UTC with the same identity and
  `root` was also rejected. This confirms a host/grant mismatch; local key
  presence and a Platform `active` row do not prove SSH login works. No
  alternate identity was tried.
- Do not rerun the older cutover command that rewrites the candidate to
  `6f6756d`; that revision predates the current approved main candidate. The
  current deployment path is the owner-approved request for `b1e2d3c`, or a
  newly reviewed bootstrap pinned to that exact revision.

## Workstation SSH and temporary-account recheck (2026-11-07 16:41 UTC)

- The existing `vps-codex-20261105` identity successfully logs in as
  `leslieghf`. Its current sudo policy allows the existing deploy and
  service-control wrappers without a password; other root commands require a
  password. The deploy wrapper reports that no approved deployment is waiting.
- An all-users `groups` scan printed `vpsssh_240aab1d : vapor-temp-ssh` once.
  Subsequent direct checks found no matching `/etc/passwd` or `getent` record,
  no home or `authorized_keys` file, and no active expiry timer; five later SSH
  sessions reported the same host machine ID and no account. The request key
  still failed authentication as that username. Group-scan output alone is
  therefore not proof of a currently usable account; the host artifact state
  was inconsistent during these checks.
- The live service probe found Caddy, Platform Server, Identity, Homepage,
  Docs, Diagnostics, Registry, Artifact, Build Cache, and the state-export
  timer active. The standalone Platform Gateway unit is absent/inactive, while
  the public `/healthz` responds `ok`; the protected admin route returns 401
  without an owner session. The public root still serves the old Vapor link
  page. No host files or services were changed.

## Failed-dispatch closure and bootstrap handoff (2026-11-07 12:46 UTC)

Historical: this section records the state before direct root SSH access and
the successful services-mode cutover described above.

- The root-console operator retired only request
  `17e1b3ee-d991-412a-aa41-eb0021d1837d` after its dispatch failure and confirmed
  there was no active deployment worker. The update was guarded by exact ID,
  version, status, dispatch state, and commit. It did not start a deployment.
- The workstation's approved key (`SHA256:rXxD/039aN25aOaqJIFBFveVlgTWd8grNcdJ9ofIg5c`)
  was rechecked against `root`; the VPS rejected the SSH login. Earlier checks
  also found `leslieghf` unavailable. No alternate credentials were tried.
- The same key was then tried under the exact managed account name derived from
  request `240aab1d-8edc-49bd-bbd1-47706983e2c3` (`vpsssh_240aab1d`); SSH also
  rejected it. The root-shell `getent passwd vpsssh_240aab1d` check found no
  local account, despite the typed helper reporting the grant as active. Treat
  that row as inconsistent state, not proof of usable SSH access; reconcile it
  after the bootstrap before requesting a fresh grant.
- The guarded one-time bootstrap is published at `sshboot` commit
  `54c2e18fdc1fbad3ccdd8f92ec75e986546fc7c1`, pinned to Platform
  `6f6756d3ec6bd120b010f3486dba3b4f7cb069a2` and Identity
  `8e1b52790fc47ba94d952175882eff1ea0565952`. It checks the retired deploy
  queue, preserves the previous server mode for rollback, and does not start
  the retired deploy timer. It has not yet been run.
- Platform source now has a root-authorized, version-guarded “close failed
  dispatch” operation and dashboard action. It is limited to approved requests
  whose dispatch definitively failed, records the operator and reason, and
  cannot close uncertain or executing work. It is committed as
  `c87aef84ed8fca9479113029250910f53fe80fe8` on
  `codex/platform-integration-20261102`, pushed, and not deployed.
- Both CI jobs for that commit passed in run `42373546825`. The full local
  `deploy/scripts/test.sh` also passed on Windows via Git Bash, including all
  Platform and service tests. The embedded admin script parsed with Node and
  `git diff --check` passed. Existing dead-code warnings remain in shared
  service crates.

## Live-surface and queue recheck (2026-11-07 10:49 UTC)

- A fresh public GET probe confirms `/healthz`, `/`, `/api/build-cache/v1/status`,
  and `/admin` return 200. `/admin/approvals`, `/admin/deployments`, and
  `/admin/ssh-access` still return 404 on the live host. The Build Cache status
  reports both supported target triples as unconfigured and not current. The
  current Platform source registers these admin routes, so the live host has
  not yet been brought to this source revision.
- The resulting temporary `host_admin` SSH request was granted through the
  root-only typed helper for eight hours. The named account and sudo access
  were verified; direct root SSH login remains disabled.
- Windows CI initially exposed two Atlas tests that require Linux atomic
  directory-symlink replacement. The tests now skip only on Windows. Commit
  `ea61cdc` passed both CI jobs in run `42365193600`.
- A review of dispatch recovery found that the existing stale `queued` →
  `unknown` normalization is intentional: a request-specific worker can be
  waiting on the shared lock after helper acceptance, while the worker only
  claims attempts within the 30-second freshness window. Converting stale
  queued work makes a manual retry refresh that window; removing this behavior
  would strand it. A follow-up source correction restores the original queue
  behavior; the correction is not yet deployed.

## Live approval surface check (2026-11-07 11:09 UTC)

- The live Platform checkout is at `072c87cb8cd90493c2a818b5493f721bfc0f6fa6`
  and the service is active. Its public `/admin/assets/admin.js` bundle does
  not contain the current Approvals inbox or SSH approval controls in source.
  This confirms the reported no-op approval button is a stale live frontend,
  not evidence that the pending request failed to exist. The updated source
  adds cursor paging for Platform deployment and SSH requests and explicitly
  identifies Identity's current 200-request endpoint cap.

## Approved deployment retry and SSH button diagnosis (2026-11-07 11:37 UTC)

- The active temporary SSH grant `240aab1d-8edc-49bd-bbd1-47706983e2c3` is
  recorded as `local_operator` / `local-root`, so the grant was issued through
  the typed host helper. This does not verify the web approval action.
- The live JavaScript bundle still contains the old `Approve temporary access`
  form. It returns without feedback if its required review checkbox is
  unchecked, and it also silently returns if `host_admin` is selected without
  the separate unrestricted-access confirmation. The current source removes
  native validation from that checkbox so the handler can show an explicit
  explanation. Commit `0c9da2e` passed `root-ci` and
  `windows-recovery-tests` in run `42367204555`; it has not been deployed.
- The already-approved request `7df36dcf-20cb-4d44-a027-09f48c4143f5` targeted
  commit `9b32a020ce26917413752d79f916b3015149a4af`, but its stored request
  predates `deployment_scope` and `service_target`. Retrying the guarded worker
  checked out that approved source, then failed closed with
  `executing request topology does not match the deployment argument`. The
  request is now failed; no running service was restarted. A fresh CI request
  using the current schema is required; do not retrofit scope into the
  previously approved record.
- After that failure, `/opt/vapor-platform-server` is checked out at `9b32a02`,
  while the active service still runs the binary built at 02:11 UTC and started
  at 10:51 UTC. Source checkout and running runtime therefore differ; the
  current admin frontend has not reached the public site.

## Fresh deploy request and renewed SSH failure (2026-11-07 11:51 UTC)

- The push of Platform `6f6756d3ec6bd120b010f3486dba3b4f7cb069a2` passed
  composed CI, and GitHub submitted a new full deployment request. The live
  Platform API rejected it with HTTP 422 because its strict request DTO does
  not recognize `deployment_scope`; this is consistent with the still-running
  older server binary and establishes the bootstrap upgrade as the current
  blocker.
- The published `main` revision and all eight pinned submodule commits pass
  `verify-deploy-source.py` locally. A read-only SSH preflight initially
  succeeded and confirmed the host is healthy, the live checkout is clean at
  `9b32a02`, Platform and Caddy are active, and 46 GiB remain available.
- The next SSH connection using the same temporary key fingerprint
  `SHA256:rXxD/039aN25aOaqJIFBFveVlgTWd8grNcdJ9ofIg5c` was rejected by
  `sshd` before a session opened. No alternate key or account was tried. The
  host-side grant row must be checked through the root-only typed helper before
  continuing the deployment.
- The operator's next guarded bootstrap stopped before making host changes
  because request `17e1b3ee-d991-412a-aa41-eb0021d1837d` remained approved even
  though its dispatch had failed. The operator then retired only that row with
  an exact ID/version/state/commit guard, after confirming no deployment worker
  was active. The row is terminally failed; its older commit
  `12df53dabe6af0802047c646e3bb6f32ae10eff9` must not be retried. The current
  bootstrap can now be retried against the already-published `6f6756d3` source.

## Recovery preflight and one approval surface (2026-11-07 09:34 UTC)

- Fresh-host recovery now validates the complete bundle and checks its Ubuntu
  release, architecture, and recorded Rust/Cargo toolchain before package
  changes. Platform commit `0628326` passed CI run `42359333223`.
- Pending deploy, SSH, and Identity permission decisions now have one browser
  surface: Approvals. Deployments omits pending requests but retains dispatch
  retry/reconciliation; SSH and Identity pages show pending requests read-only
  and link back to their inbox section. Owner APIs and their record ownership
  are unchanged. Identity change `8154a79` is pinned by Platform `fec75f2`.
- The full local Platform test recipe passed after these changes: 85 Python
  tests (10 skipped) plus all Rust service tests. Platform CI run
  `42360322183` passed both `root-ci` and `windows-recovery-tests`.
- No VPS deployment was performed.
- The failure email most likely corresponds to run `42348443012`: `root-ci`
  failed because the Linux helper called `record_stopped_deploy_failure`
  without its module qualifier. Commit `d19c9be` fixed it; run `42348670442`
  and later runs passed.

## Responsive admin tables (2026-11-07 09:48 UTC)

- Wide Build Cache, Artifact, Registry, and Identity admin tables now sit in
  named, keyboard-focusable horizontal-scroll regions. The shared shell gives
  focused regions the same visible focus outline as other controls.
- Service commits: Build Cache `e47817a`, Artifact `647a1ea`, Registry
  `4b0951b`, and Identity `8e1b527`; each was pushed to its existing feature
  branch and is pinned by the Platform worktree.
- Local service tests passed: Build Cache 8 unit + 7 contract, Artifact 4,
  Registry 5, Identity 38. Platform CI runs `42360322183` and `42360410112`
  passed both jobs for the preceding approval-surface changes. Final responsive
  commit `5e1d089` passed both jobs in CI run `42361324829`.
- This closes the static wide-table accessibility gap. A hands-on narrow
  viewport and assistive-technology review remains outstanding. No VPS
  deployment was performed.

## Recovery integrity and USB retention checks (2026-11-07 10:20 UTC)

- Restore now verifies the detached archive sidecar before parsing the archive:
  the sidecar must be one canonical SHA-256 entry naming the selected archive.
  A valid digest attached to another filename is rejected.
- USB retention fixtures cover the destructive budget path. They verify that
  pruning keeps the current content-verified archive, removes only the oldest
  verified history needed to fit the configured archive-payload budget, and
  refuses without deleting history when unindexed/manual data leaves no room.
- The sync report now calls its byte field `retained_archive_payload_bytes`;
  it excludes sidecars, extracted kits, and other USB contents. Build docs also
  distinguish developer-local compilation, approved primary-host service
  deployment, and the not-yet-implemented target-snapshot publisher.
- Focused local validation passed: 35 recovery/deployment Python tests (4
  platform-specific skips), Python syntax compilation, diff checks, and the
  Windows USB-sync PowerShell suite. Platform CI run `42363390368` passed both
  `root-ci` and `windows-recovery-tests` for commit `96eee5d`.
- These changes are source-only; no production deployment occurred. Direct
  root SSH inspection was rejected by the VPS, so live host status remains
  unverified. The disposable clean-host recovery drill also remains outstanding.

## Public read-only probe (2026-11-07 10:21 UTC)

- `GET /healthz`, `/`, and `/api/build-cache/v1/status` returned HTTP 200.
- The public Build Cache response reports both supported target triples as
  `configured: false` and `current: false`; no current target snapshot is
  available. This confirms the snapshot orchestration gap is live, not only a
  source-plan concern.
- Unauthenticated `GET /admin` returned 200, while direct requests to
  `/admin/approvals` and `/admin/deployments` returned 404; the protected
  `/api/admin/operations` returned 401. These are route responses only, not a
  successful authenticated dashboard workflow check.
- The reported failed deployment helper response
  (`arguments are not accepted for interactive operations`) is explicitly
  classified by current source as a helper-protocol mismatch. The installed
  helper revision could not be checked because SSH authentication was rejected;
  a full deployment installs the service and its helper from the same pinned
  checkout, but has not been performed.
- Root SSH authentication was rejected during the same pass. No service state
  or configuration was read, and no deployment or mutation was attempted.

## Recovery and reconciled-deploy hardening (2026-11-07 06:45 UTC)

- Recovery validation now rejects more than 100,000 archive entries, any
  regular file larger than 2 GiB, or total expanded regular-file data above
  8 GiB before restore extracts the archive.
- If an executing deployment worker has stopped without recording its result,
  reconciliation now records `dispatch_state=unknown` with an explicit detail
  instead of leaving the contradictory `claimed` state. The deployment request
  remains closed as failed and requires a fresh approval; the deployment effect
  itself is still explicitly unknown.
- The deployment request card reflects that distinction instead of labeling an
  unknown execution outcome as a confirmed deployment failure.
- Regression tests cover each recovery bound and the stopped-worker lifecycle
  transition. The full local Platform test recipe passed after these changes.
- CI run `42348443012` found a Linux-only helper qualification error that the
  Windows build could not expose; commit `d19c9be` corrected it. CI run
  `42348670442` passed both `root-ci` and `windows-recovery-tests` for that
  revision.
- SSH read-only probes using both the root key and the same key for `leslieghf`
  were rejected. No VPS changes were made.
- The follow-up UI wording passed the full local recipe and CI run
  `42349045185`. The status-record follow-up also passed both jobs in run
  `42349265994`.

## Approval dispatch lifecycle follow-up (2026-11-07 06:30 UTC)

- The deployment approval endpoint and worker now record distinct queued,
  claimed, and completed dispatch states. The endpoint's final write is guarded
  by the original request version and cannot overwrite a worker that has already
  claimed or completed the deployment. If the worker advances first, the
  approval response returns the current authoritative state.
- A worker may claim only a fresh dispatch attempt. Stale queued and dispatching
  attempts normalize to `unknown` and become retryable; executing/claimed work
  is excluded from stale recovery. The request history exposes the new worker
  states and the deployment UI reports terminal failure consistently.
- Local verification passed: all 76 Python tests (9 platform skips), the Rust
  Platform test suite (63 unit tests plus 3 CLI integration tests), all
  workspace Rust tests through `deploy/scripts/test.sh`, and the shell,
  Python-syntax, Atlas, embedded-JavaScript syntax, and changed-document local
  link checks. Existing Rust dead-code warnings remain.
- GitHub Actions run `42347119929` passed both `root-ci` and
  `windows-recovery-tests` for commit `620c5b07632fe12cc295b56398b44c697a1676aa`.
- This is source-only; no VPS deployment was performed. The follow-up inbox
  feedback and documentation cleanup are newer local changes and still need
  their own CI run before they are a validated candidate.
- The latest read-only SSH attempt with the designated workstation key was
  rejected by the VPS (`Permission denied (publickey)`). No live files or
  services were changed; the VPS remains on its prior deployed revision.
- The inbox now preserves and explains the dispatch stage returned by
  deployment approval, with a direct link to Deployments. The November 1–3
  workstream docs now point readers to the current service topology and
  approval sources instead of presenting superseded assumptions as current.

## Prior recovery candidate validation (2026-11-07 06:10 UTC)

- The latest validated candidate is
  `cc80245b5d33100bcadd4cbe603ba6e451f7d51a` on
  `codex/platform-integration-20261102`. GitHub Actions run `42345635963`
  passed both `root-ci` and `windows-recovery-tests`.
- The full local recipe passed with 75 Python tests (9 platform-dependent
  skips), all Rust unit/integration/doc tests, shell syntax checks, and the
  Windows USB fixture suite (8 scenarios).
- The new exports carry their source-pinned recovery entrypoint and metadata
  helper; USB sync verifies the kit hashes and explicitly marks the latest kit
  unavailable when syncing an older archive. Recovery now requires Steam and
  dashboard readiness, plus GitHub browser readiness only when at least one
  GitHub OAuth credential is configured. Missing GitHub configuration flags
  fail closed.
- The prior Windows failure on `be4c5e9` was caused by PowerShell combining the
  Windows and Git `tar.exe` paths. `3265b3f` selects a single executable, and
  its native Windows CI run passed.
- These are source and CI results only. No live VPS deployment, real USB copy,
  or clean-host restore drill was performed.

## Latest follow-up (2026-11-07 03:24 UTC)

- The temporary SSH expiry unit `vapor-codex-access-expire.service` is failed
  since 00:44:16 UTC. Its transient `ExecStart` is
  `/usr/sbin/deluser leslieghf vapor-temp-ssh`, and its main process exited
  with status 255. Read-only host checks confirmed `leslieghf` remains a member
  of `vapor-temp-ssh` and the temporary public key remains in that account's
  `authorized_keys`; expiry therefore did not complete. The current SSH
  operator cannot read the system journal or perform root cleanup without
  interactive authentication, and the workstation's root key is rejected.
  Root-side journal inspection and revocation are required before treating this
  grant as expired.
- The failed transient unit is not created by the tracked per-grant SSH helper
  or persistent grant-expiry units described in the current design. It is an
  out-of-band account/group expiry path; reconcile it with the tracked grant
  lifecycle rather than treating its failure as a regression in the current
  helper.
- CI run `42333623616` for branch commit `9255757` completed successfully,
  including `root-ci`. The dispatcher ID-reporting change is source-only and
  is not deployed.

## Directly verified live state (2026-11-07)

This section supersedes earlier status summaries below. The host was inspected
over root SSH, and local and public health checks were run after recovery.

- A later verification checked the 2026-11-07 `9b32a02` candidate. GitHub
  Actions run `42331660686` completed both composed Platform CI and deploy
  request submission successfully. The request is approval-gated; this check
  did not dispatch or install it. Public `/healthz` and `/admin` return HTTP
  200, while `/admin/approvals`, `/admin/deployments`, `/admin/ssh-access`,
  `/admin/hosting`, and `/admin/history` return 404. The candidate admin
  routes are therefore not live yet. `/api/admin/operations` returns 401
  without an authenticated owner session.
- The current Windows SSH key reaches the VPS as the non-root operator account;
  the root key currently available to this workstation is rejected. The
  operator's passwordless sudo allowlist exposes only the approved-deployment
  dispatcher and allowlisted service restarts. The initial route probe was
  read-only.

- At 03:11 UTC, the approved `vapor-ssh-deploy` dispatcher returned
  `Queued 2 approved deployment request(s).` It reports only a count, not the
  request IDs. A subsequent read-only check found no active systemd jobs; the
  only retained failed deployment unit was still the earlier
  `bdf86cf7-f6b5-4681-bc9d-ee644d2ca5a3` failure from 01:03 UTC. The restricted
  account cannot read the Platform database or other users' system journal,
  so the two queued IDs and their terminal outcomes could not be correlated.
  Public `/healthz` and `/admin` still returned 200, while the candidate
  `/admin/approvals`, `/admin/deployments`, `/admin/ssh-access`, `/admin/hosting`,
  and `/admin/history` routes remained 404. Deployment of the candidate is
  therefore unverified; this observation does not establish success or failure.

- The approved deploy request
  `bdf86cf7-f6b5-4681-bc9d-ee644d2ca5a3` targeted `a8462945af4b16befd9e94964917cad15e33cfd6`.
  Approval persisted cleanly, but its deployment failed during startup. The
  request is terminal with `status=failed`; it was not re-used.
- Recovery exposed two startup defects: the shared Platform unit imported only
  `platform.env` even while configured for monolith mode, and the SSH audit
  trigger migration could fail when the trigger already existed during startup.
  The unit configuration and migration were corrected in commit
  `09ae2af441e55911c20087ed8922eb8cab01b8cb`.
- The Platform service was rebuilt from that exact commit on the VPS as a
  narrowly scoped recovery. It was running in `monolith` mode with zero
  restarts after recovery. The next approved request,
  `5338a3de-fd38-4c89-94ed-5afba23722ab`, targeted `95a8bbcdfb6cabcf21352fd25285c3eaf87638fe`.
  Dashboard dispatch failed twice with a generic helper error, so root started
  that exact approved request's one-shot worker directly. The worker built and
  deployed the approved commit successfully in the existing `monolith` mode.
  The full local health script then passed, including the public
  Atlas-through-Caddy route, and SQLite integrity returned `ok`.
- The request record is `status=succeeded`, while its dispatch substate remains
  `failed` because the dashboard helper did not queue it; the direct root start
  was not represented in that substate. This is a known record/presentation
  inconsistency, not a failed deployment. The older binary discarded the
  helper's structured failure detail. The deployed `072c87c` binary now logs a
  bounded structured helper failure reason for future dispatch attempts.
- A follow-up request workflow for Platform commit `f78f236` passed its full
  composed Platform CI job, then failed while submitting the new request:
  the live API returned HTTP 422 because its deployed request schema does not
  recognize `server_mode`. This confirms the host is still running the older
  request contract and cannot yet accept the target-pinned deployment request.
  No request for `f78f236` was created by that workflow.
- The next workflow for `072c87c` also passed composed CI and failed at request
  creation with the same HTTP 422. Its submission occurred at 02:08 UTC, before
  the bootstrap deployment below completed; it is not evidence that the updated
  server still rejects the new request shape.
- After that workflow failed, the exact CI-tested `072c87c` root commit was
  installed through the owner-authorized bootstrap path, retaining `monolith`
  mode. The deployment completed and recorded a successful operation. The live
  checkout is at that commit with its pinned submodules; the Platform service
  is active, local and public `/healthz` returned `ok`, public `/admin` returned
  HTTP 200, and the SQLite integrity check returned `ok`. No services-mode
  topology change was performed.
- The deployed API now contains the target-aware request schema. A new main CI
  run is needed to verify that it accepts `server_mode` and creates the pending
  services-mode approval request. Such a request must remain pending for owner
  review; this bootstrap did not approve or dispatch the topology switch.
- A pre-repair SQLite backup is retained at
  `/var/backups/vapor-server/platform-pre-repair-20261107T010500Z.sqlite3`;
  the failed candidate binary is retained under
  `/var/backups/vapor-server/platform-failed-candidate-20261107T011200Z/`.
  The backup is protected as root-only state.
- GitHub Actions CI run `42323052997` for recovery commit `09ae2af` completed
  successfully. The live source checkout has since advanced to `95a8bbc` via
  the approved request above; the prior failed request remains recorded as
  failed.
- The previous successful deploy of `6df33ec` was on 2026-11-06. The earlier
  CI failure on `d5e01da` was a recursive-checkout failure because the pinned
  Identity commit was not fetchable from its remote at that time; the later
  successful recursive checkout proves that pin is now published and fetchable.
- The VPS contains retained `.tar.gz` state archives, but the live backup
  directory does not currently contain the new export's checksum sidecars,
  `index.json`, or `latest.json`. The new index/verification behavior is source
  only until the candidate is approved and deployed. The owner's USB was not
  connected during this check, so USB sync and restore remain unverified.

Last verified legacy baseline: 2026-07-25 Europe/Berlin via GitHub Actions
deploy run `30139280612` and CI run `30139280620`.

This records the last verified live VPS state, not the current source tree.
Changes made after that deployment are not considered deployed until a later
VPS check updates this record.

## Historical VPS baseline (verified 2026-07-25)

- OS: Ubuntu 26.04 LTS.
- Panel: none; Plesk is not used.
- Reverse proxy: Caddy.
- Service manager: systemd.
- Automatic deployment: `vapor-deploy.timer` checks the configured branch.
- GitHub deployment trigger is configured and verified through a restricted VPS
  deploy user. Branch protection is not configured yet.
- Intended public host: `vapor.ghf-studios.site`.
- Temporary pre-DNS access: HTTP fallback may be configured directly against the
  VPS while the domain registration is pending.
- Firewall: UFW enabled with inbound SSH, HTTP, and HTTPS only.
- Deploy root: `/opt/vapor-server-root`.
- State root: `/var/lib/vapor-server`.
- Server-local config: `/etc/vapor-server`.

## Recovery and cutover status (reported 2026-11-03)

This section supersedes the historical timer and deploy-root statements above
for current planning. It is based on the recent uploaded VPS reports and the
latest safe cutover attempt, not a fresh direct SSH inspection.

- The public site returned HTTP 200 and Caddy was active after its config
  permissions were repaired.
- The old periodic deploy timer had been disabled. The latest cutover retry
  stopped at preflight because the existing Platform checkout was not at the
  revision hard-coded into that script; it made no changes.
- `/opt/vapor-platform-server` is an existing Git checkout and must be treated
  as preserved state, not as an empty legacy directory. Keep its current
  source, state, service-local configuration, and Caddy configuration backed up
  through the next cutover.
- The request-triggered deployment flow and temporary SSH grant system are
  source work only until a new approved revision is installed and verified.
  No temporary SSH account or grant should be assumed to exist.
- The Platform source already composes Build Cache and exposes its admin
  inventory. The VPS-hosted Linux and Windows snapshot builder orchestration
  remains an outstanding infrastructure phase; the present Build Cache
  contract is not proof that either target snapshot is being produced.
- Approved Platform service deployment remains independent of that future
  snapshot/worker lane: it builds the selected pinned service locally on the
  primary host with `cargo build --release --locked`. Snapshot unavailability
  therefore does not block current service deployment or fresh-host recovery.

An additional public read-only probe on 2026-11-03 returned `200 ok` for
`/healthz`, but `404` for `/admin/operations`, `/api/admin/operations`,
`/api/build-cache/v1/status`, and `/docs`. This is consistent with the new
composed Platform/admin routes not being live yet; the generic health route
alone does not identify the running revision. This probe did not access the
VPS directly and does not supersede the need for an authenticated host report.

The one-time cutover should therefore be an explicit migration from the
preserved Platform checkout to an exact approved root commit, retaining the
existing path and rollback copy until the dashboard, request-triggered deploy,
health checks, and Build Cache service all verify. It must not restore or enable
the retired polling timer.

Secrets are intentionally not recorded here. Admin tokens live only in
server-local env files under `/etc/vapor-server`.

## Recovery and optional worker direction (design, not deployed)

The intended recovery bundle is being changed from a state-only archive to a
consistent primary-host recovery set: service state, `/etc/vapor-server`
configuration and credentials, selected Caddy/systemd routing files, and exact
source/submodule pins. The archive will be intentionally unencrypted and copied
hourly to an owner-held 128 GB USB drive by a Windows-side SSH pull task. The
root SSH private key remains separate. Existing bundles do not satisfy this
target and must not be described as full-platform backups.

A future second Strato VPS is optional build and replica capacity. It will not
host public routes or own primary service state. Build/deploy and recovery must
remain usable when it is offline; its actual role depends on the selected
server's specifications. No worker or USB restore drill has yet been verified.

## Current source-only progress (2026-11-07)

The worktree now contains an independent-mode Platform admin gateway slice:
Platform remains on loopback port 7110, serves its own admin pages and health
route, and forwards only fixed Identity, Workshop, Artifact, and Build Cache
admin page prefixes to their owning service ports. The services-mode Caddy
template routes `/admin*` and `/api/admin*` to that gateway; service APIs stay
on their existing owner routes. The deployment path now builds, installs,
enables, restarts, and health-checks the gateway in services mode, and the
Platform unit reads only its own environment file. The deployment health check
also probes each fixed owner-admin page so a service returning healthy cannot
hide a broken shared-admin route.

The gateway implementation is present in the deployed binary, but the live
host still runs `monolith` mode; independent service routing and gateway
behavior have not been activated and runtime-verified. The gateway still uses
the existing shared HTML shell middleware for service pages; replacing that
HTML marker/buffering contract with the explicit shared-layout contract in the
approved design remains open. The Windows USB sync now takes a per-destination
mutex before stale-partial cleanup, preventing overlap between the hourly task
and manual catch-up. A fixture suite covers initial copy/catch-up, pointer
mismatch, bad archive hashes, and overlapping runs; it passed locally on
PowerShell 7.6.9. CI is configured to run it on Windows. This does not verify
the owner's removable drive, scheduled task, or a fresh-host restore. Fresh-host
bootstrap/restore remains open; the current recovery scripts are not a proven
rebuild path.

The current source pins the deployment topology (`services` or
`monolith`) inside each immutable approval request. New CI requests target the
approved independent-services architecture; the dashboard displays the target
and refuses approval of legacy requests without one; the worker verifies and
deploys only the recorded target. The targetless-retry guards in the API and
dashboard and the explicit target check in the root helper are included in
`072c87c`. That exact commit is now deployed in `monolith` mode after the old
API rejected `server_mode`; the 02:08 UTC CI failure predated that installation.
The next CI submission is the verification point for the updated API contract.

## Verified deployed source

These are the runtime/service commits verified on the VPS. Documentation-only
commits in `Vapor-Platform-Server` may be newer than the runtime-impacting commit
recorded here.

```text
Vapor-Platform-Server       a2a9592
Vapor-Homepage-Server   a41aedc4180792d5561a8e3bf12a1383e172c1ea
Vapor-Docs-Server       63ff610
Vapor-Identity-Server   ca9e61e
Vapor-Diagnostics-Server 058c403
```

## Verified runtime state (historical snapshot, 2026-07-25)

- `caddy.service` is active/running.
- `vapor-homepage.service` is active/running.
- `vapor-docs.service` is active/running.
- `vapor-identity.service` is active/running.
- `vapor-diagnostics.service` is active/running.
- `vapor-deploy.timer` is enabled for periodic deployment checks.
- The first timer-triggered `vapor-deploy.service` run completed successfully
  against `main`.
- Restricted deploy-user SSH can trigger `vapor-deploy.service`, and the
  resulting service run reports `success`.
- GitHub Actions deployment secrets are configured. Push-triggered workflow run
  `30126015161` successfully requested the VPS-owned deploy service, printed
  deploy service status, and ran public pre-DNS HTTP smoke checks.
- GitHub Actions workflow-dispatch run `30126858680` successfully verified the
  deployment path after automatic state-export timer installation.
- GitHub Actions push-triggered deploy run `30139280612` successfully deployed
  root commit `a2a9592`, printed deploy-service status, and ran public HTTP
  smoke checks. Companion CI run `30139280620` ran repository-local QA against
  recursive submodules.
- `vapor-state-export.timer` is enabled/active. It produced
  `/var/backups/vapor-server/vapor-server-state-20260724T211010Z-3ec87f6.tar.gz`
  with root-only permissions. The bundle manifest reports
  `secrets_included = false` and excludes `/etc/vapor-server`.
- Local health/status checks pass on ports 7111, 7112, 7113, and 7114.
- Public pre-DNS HTTP health/status checks pass through the fallback route.
- `/etc/vapor-server/root.env` preserves non-secret deployment settings for
  timer-driven runs, including the temporary pre-DNS HTTP fallback host.
- SSH remains reachable through key authentication after hardening.
- Identity uses SQLite and has been initialized through the local admin-token
  endpoint.
- Identity database files are owned by `vapor:vapor` with restrictive file
  permissions.
- Identity auth readiness endpoint is deployed. GitHub OAuth browser/device
  configuration is present and reports ready. Steam Web API key configuration is
  present and reports ready.
- Top-level `/login`, `/logout`, and `/admin` routes are routed to the identity
  service. `/login` is the browser login/register page. `/admin` is publicly
  reachable as a locked shell.
- Identity has Steam-anchored browser profiles through Steam OpenID, GitHub
  browser OAuth linking stubs, GitHub Device Flow/API seams, 5-minute auth
  attempts, and 5-minute dashboard sessions. The dashboard only renders
  privileged identity data/actions when the request carries a non-expired root
  session for a profile with linked Steam and GitHub identities plus the `root`
  role.
- Identity source has been split out of the previous monolithic `main.rs` into
  focused modules for config, route handlers, persistence, provider
  verification, profile/session logic, and shared utilities.
- Identity has a server-local role-grant operator route and script:
  `POST /v1/admin/roles/grant` and
  `deploy/scripts/grant-identity-role.sh`. The route grants `root` or
  `content-developer` only by requiring both external identities: SteamID64 and
  GitHub login. Those identities must already be linked to the same internal
  profile row. The internal profile id is not accepted as grant authority. The
  route accepts either the server-local bootstrap token or a non-expired root
  dashboard session.
- Identity has role-revoke routes and scripts:
  `POST /v1/admin/roles/revoke`, `POST /admin/identity/roles/revoke`, and
  `deploy/scripts/revoke-identity-role.sh`. Revocation uses the same linked
  SteamID64 plus GitHub-login target model as grants. The service refuses to
  revoke the last active `root` role and rejects attempts to revoke
  `content-developer` from an active root profile because `root` implies
  developer capability.
- Identity has recent audit visibility through `GET /v1/admin/audit`, the root
  dashboard, and `deploy/scripts/list-identity-audit.sh`. Authenticated VPS
  smoke verified the audit helper works without printing the server-local admin
  token.
- The root dashboard has first functional role-grant and role-revoke controls
  backed by the same validation paths as the JSON API. Local tests cover root
  effective-role display, last-root revoke refusal, no-op revoke audit behavior,
  and audit listings that avoid exposing internal profile ids.
- Internal profile ids are not shown in the browser dashboard or protected
  profile listing; they remain database join keys only.
- Public unauthenticated requests to role-grant, role-revoke, and audit routes
  reject with `401`.
- Public docs and diagnostics status probes are deployed:
  `/docs/status` and `/api/diagnostics/status`.
- Public unauthenticated contract checks cover docs write/export rejection,
  diagnostics protected read rejection, identity admin rejection, and the
  removed legacy root-grant route.
- The removed `/v1/admin/root/grant` compatibility route returns `404`.
- `deploy/scripts/configure-identity-auth.sh --status` waits for the identity
  service after restart instead of immediately failing on a bind/readiness race.
- `root` is a role/group on normal Steam-anchored profiles, not a separate
  account type. GitHub does not create standalone player profiles. Effective
  authorization treats `root` as implying `content-developer`.
- The old server-local dashboard password remains present in
  `/etc/vapor-server/identity.env` for compatibility/readiness visibility, but
  it is no longer the dashboard authorization model.
- Temporary HTTP-by-IP identity cookies are explicitly configured with
  `VAPOR_IDENTITY_PUBLIC_ORIGIN=http://82.165.77.104`,
  `VAPOR_IDENTITY_COOKIE_SECURE=false`, and `VAPOR_IDENTITY_COOKIE_PATH=/`.
  Move this to secure HTTPS cookies once DNS is active.
- `deploy/scripts/configure-identity-auth.sh` and
  `deploy/scripts/smoke-identity-auth.sh` are deployed on the VPS.
- Curated Vapor docs are deployed through the public HTTP docs route: 410 files,
  8,739,662 bytes uncompressed, served under `/docs/`.
- Docs source now uses a single current-tree publication model. Existing
  deployed docs state may still be the previously uploaded curated docs bundle
  until the next authenticated docs publication.
- Diagnostics source now exposes one structured JSON report API with stronger
  redaction, collision-resistant run ids, and aggregate quota support. Public
  smoke verifies protected reads fail closed without authorization; an
  authenticated diagnostics upload smoke has not been run on the VPS in this
  verification pass.
- An older diagnostics smoke run was previously uploaded and verified to redact
  obvious secret tokens on disk.
- A root-only state export bundle was created under `/var/backups/vapor-server`
  and verified to contain `/var/lib/vapor-server` state plus a manifest, while
  excluding `/etc/vapor-server` env/token files.
- Restore source now validates state-bundle structure before extraction and
  before stopping services. A disposable-server restore rehearsal remains
  pending.

## Remaining external dependency

DNS for `vapor.ghf-studios.site` still needs to point at the VPS once the domain
registration is active. Public HTTPS/certificate issuance depends on that DNS
being correct.

The temporary HTTP fallback should be removed after DNS and HTTPS are verified.

## Temporary account state rechecked (2026-11-07, about 16:46 UTC)

- Repeating the all-users `groups` scan through the working `leslieghf` SSH
  connection did print `vpsssh_240aab1d : vapor-temp-ssh`; this confirms that
  the account was present at the instant of that scan. The same key was then
  rejected for that login. Within about a minute, direct `grep`, `getent`, and
  `groups` checks found no account and no `/home/vpsssh_240aab1d` directory,
  consistently across three checks in one SSH session.
- The host still showed the grant's full UUID expiry timer active, scheduled
  for 2026-11-07 19:05:58 UTC. The corresponding service was inactive; its
  `ExecMainStartTimestamp` was empty. The shortened timer name previously
  queried was not the installed full-UUID unit. The account's disappearance
  before the timer's scheduled expiry remains unexplained; this is a lifecycle
  inconsistency, not evidence that the user supplied the wrong key. No host
  state was changed.
- Source review confirms the normal revoke/expiry helper stops and disables
  the timer before deleting the account, then removes the timer units. The
  observed combination (account absent, timer active) is therefore not the
  expected completed helper path. Root-owned Platform DB and system journal
  evidence are still required to distinguish interrupted or out-of-band
  cleanup from a helper/version mismatch.

## Root SSH recovery and stale-grant cleanup (2026-11-07, 17:20 UTC)

- The owner installed the workstation's designated root public key. An SSH
  login as `root` succeeded, confirming this key is now usable for the
  temporarily authorized recovery window. The private key remains only on the
  workstation and is not recorded here.
- Narrow host logs confirm request
  `240aab1d-8edc-49bd-bbd1-47706983e2c3` was granted at 11:05 UTC and its
  temporary account was created at 11:06 UTC. The account successfully ran
  read-only helper commands at 11:07 and 11:43 UTC; SSH login attempts at
  11:49 UTC and later were rejected because the account was absent.
- `/etc/passwd` and `/etc/shadow` timestamps remain at the account creation
  second (11:06:20 UTC), and the available journal/auth logs contain no
  deletion event for this account. Auditd is inactive. The specific cause of
  disappearance is therefore still unknown; the logs do not support attributing
  it to a particular operator or cleanup path.
- The request row still reported `active` while its account and home directory
  were absent. Its full-UUID expiry timer was enabled for 19:05:58 UTC, but the
  expiry service had never run. The separately observed failed transient
  `vapor-codex-access-expire.service` attempted to remove `leslieghf` from a
  group at 00:44 UTC, more than ten hours before this grant was created, and
  failed with an invalid group-name error; it does not explain the later
  account disappearance.
- The installed typed operator's `ssh revoke` path was then used to reconcile
  this unusable grant. It completed successfully, removing that grant's managed
  timer/key/sudo policy/account artifacts and recording the grant as revoked.
  This did not modify the separate `/root/.ssh/authorized_keys` recovery key.
- CI for the current source branch through `e969970` is green. No application
  deployment or service restart was performed during this host diagnosis.
