# Vapor server deployment status

## Current verified state (2026-11-09 01:47 UTC)

- The repaired state-only USB sync completed successfully at
  `2026-11-09T01:22:54Z`, then a fresh-destination USB run completed at
  `2026-11-09T01:25:23Z`. The fresh run fetched all eight generations from the
  VPS (38,389,881 bytes), verified archive, sidecar, and recovery-kit hashes,
  and reported success to Platform. The installed scheduled-task script
  matches the tested repository copy. Existing recovery data was preserved;
  the fresh run used a separate USB destination.
- The latest owner-held archive is
  `vapor-platform-state-20261109T010116Z-34f63ec46441.tar.gz` with SHA-256
  `edd21e88ec907b1108914f15561e0201629af14c4b865cd29da876c3bc9a53e9`. Its
  archived Platform source is `78722c2641b42bfdf493d45b6b92461dccd3e87f`;
  the separately pinned recovery-kit source is
  `cbcf388536fdce8c18e852a8950fd6cabed5acee`. This archive was exported after
  the live deployment completed at `2026-11-09T00:47:09Z`.
- The VPS deployed-source map records composition
  `78722c2641b42bfdf493d45b6b92461dccd3e87f` for Platform and all eight service
  targets. Platform reports the deployment succeeded. Platform, Homepage,
  Docs, Identity, Diagnostics, Registry, Artifact, Build Cache, and Caddy were
  all active during the latest check; `systemctl --failed` returned no failed
  units. The three old failed systemd deployment units were reset after their
  outcomes were confirmed in Platform history; the history records remain
  intact.
- Public `/`, `/downloads`, `/docs/`, `/login`, `/dashboard`, and
  `/operator/login` return HTTP 200. All 29 same-origin links in the Docs index
  resolved successfully. Unauthenticated `/admin`, `/admin/approvals`, and
  `/admin/recovery` return HTTP 401, as required; authenticated operator
  workflows still need an end-to-end browser check.
- Build Cache currently reports both supported targets as unconfigured and
  without a current snapshot. The optional worker path still lacks the
  canonical ecosystem/toolchain lock. Local source builds remain the available
  fallback; no clean-host restore drill has been completed yet.
- Platform `main` is at `081d60b32654eb8bac7855310221dbe8d7cbd615`. Its Windows
  CI had falsely failed after the state-only USB test correctly caught an
  unreachable archived revision: Git's expected exit code `1` leaked from that
  handled validation into PowerShell's final process status. The predicate now
  clears the native exit code before reporting the expected validation error.
  The test passes locally with exit code `0`; the pushed composed run
  `42547112705` passed both Linux root CI and all Windows recovery tests.
  Deployment submission was correctly skipped for this local-recovery-only
  change, so live composition remains `78722c2641b42bfdf493d45b6b92461dccd3e87f`.
- The emergency web console remains disabled by default: its enable flag is
  absent from the live Platform environment, and its enable guard also requires
  an isolated admin origin. The unauthenticated console route returns `401`.
- The archive and recovery kit are checksum-verified on USB, but archive
  verification is not a substitute for a fresh-host restore drill. Build Cache
  still reports both target triples unconfigured with no current snapshots;
  the optional worker and canonical ecosystem/toolchain lock are not yet in
  place. Local source builds remain the fallback.

## Previous checkpoint (2026-11-08 22:05 UTC)

- The primary VPS is running the independently managed Platform, Homepage,
  Docs, Identity, Diagnostics, Registry, Artifact, and Build Cache services.
  The deployed Platform checkout is pinned to
  `20d1d234b6b2886250fc2ade4c3d016aebbdf708` in `services` mode. All eight
  Vapor services were active during the check; the state-only export timer is
  enabled and active, while the legacy full-host export timer is disabled.
- The public homepage, `/downloads`, `/docs/`, `/login`, `/dashboard`, and
  `/operator/login` returned HTTP 200. Unauthenticated `/admin` returned the
  expected 401. This verifies route availability, not an authenticated browser
  session or the interactive admin workflows.
- The latest owner-held state-only snapshot is
  `E:\VaporRecovery\State-Snapshots\vapor-platform-state-20261108T212902Z-71cc538943e2.tar.gz`
  (4,798,388 bytes; SHA-256
  `8f0267596f2af29f00b4ce3bb3a89024787f12c0f622ac5dcf7a265cbf52e024`). Its
  manifest and matching recovery kit are pinned to the deployed source
  revision above. The archive validator passed, the USB index contains four
  state-only generations (19,190,190 bytes), and the latest pointer/checksum
  match the archive.
- The hourly `Vapor USB Recovery Sync` now uses the state-only path. The latest
  sync record reports success, all four generations verified, and the recovery
  kit present. The old `E:\VaporRecovery\last-sync.json` belongs to the legacy
  full-host sync and is not authoritative for state-only synchronization.
  Fifty-three legacy full-host archives (254,801,651 bytes) still remain on the
  USB; the disabled legacy timer will not create more. They are not part of the
  state-only set.
- The state snapshot is structurally valid and owner-held, but a clean-host
  restore has not been exercised. A disposable second host is still needed to
  prove the complete source rebuild and state import without touching the live
  VPS.
- The VPS has one vCPU, 1.8 GiB RAM, and no swap. It remains the local build
  fallback for service deployments, but is unsuitable for a concurrent VM
  worker. Build Cache reports `configured=false` and `current=false` for both
  supported targets; no promoted snapshots or optional worker are available.
  Local source builds remain the fallback.
- Platform `main` is at
  `e1e0c8a3782cf3b0ec2f5a0f56b6414574253b90`; both Windows recovery tests and
  Linux composed CI passed. The live VPS remains on
  `20d1d234b6b2886250fc2ade4c3d016aebbdf708`; the intervening changes are
  deployment workflow and offline recovery tooling, so no service rollout was
  needed. Platform main's fresh-host recovery entrypoints now default Cargo to
  one build job, with an explicit validated override for larger machines. The
  current owner-held state bundle and embedded recovery kit remain pinned to
  `20d1d234...`, so they do not yet contain this adjustment; a newly
  source-pinned bundle is needed before the USB recovery path benefits from it.
- The workflow correctly skipped its deploy-request submission for that
  recovery-only commit. One earlier docs-only request remains pending:
  `bdf04e61-2fcd-45ec-a556-7ff061ca45ea`, for commit
  `9a297fca6e9b2320c224ef9dd1f4d498c44af2bf`. It is not a live deployment;
  reject it through Platform Approvals before reviewing later requests so it
  cannot hold the approval-order queue open.

## Historical checkpoint (2026-11-08 11:09 UTC)

- Platform `31f192a4f0c6bb54e6c581e8b2d6471c7c951ab7` is live in `services`
  mode. Its approved request `1ead570a-756e-4eef-aa1e-fc25d3239f01` completed
  successfully at `2026-11-08T11:07:39Z`. The host's deployed-source map pins
  all eight service targets to this composition revision and their exact source
  commits.
- Caddy and the Platform, Homepage, Docs, Identity, Diagnostics, Registry,
  Artifact, and Build Cache services are active. Direct health checks on ports
  7110–7117 returned HTTP 200. Public `/`, `/downloads`, `/docs/`, and
  `/api/build-cache/v1/status` also returned HTTP 200.
- The Build Cache status response currently reports both supported targets as
  `configured=false` and `current=false`: `x86_64-unknown-linux-gnu` and
  `x86_64-pc-windows-gnullvm`. The service is reachable, but there are no
  promoted snapshots; the primary VPS remains the deployment-build fallback.
- The latest host recovery archive is
  `vapor-platform-recovery-20261108T110848Z-31f192a4f0c6.tar.gz` (4,816,179
  bytes; SHA-256
  `6fe0c90bc6993d2f94a919fd47e6300b48d3f00e630393abe6e982cdae616088`). The
  manifest includes the recovery kit and matches the latest pointer.
- The owner-held `E:\VaporRecovery` sync completed successfully at
  `2026-11-08T11:09:06Z`. It copied the current archive, re-hashed all 44
  indexed archives, reported zero unverified archives and 211,370,133 bytes of archive
  payload, and validated the embedded recovery kit against source commit
  `31f192a4f0c6bb54e6c581e8b2d6471c7c951ab7`. A one-time full checksum sweep
  also found no checksum failures. The hourly task is registered and its last
  result was success; the next run was scheduled for 12:00 local time.
- A direct host snapshot reports 47 GiB free on `/`, 1 vCPU, and 1.8
  GiB RAM. The VPS remains suitable for its current deployment-build fallback,
  not for a concurrent guest compiler worker.
- The two newest deployment requests were both successful. Older approved
  records with failed dispatch remain in the database as history; there is no
  active request-specific deployment worker. Do not retry those stale records
  as if they were pending work.
- The owner-held archive copy is now verified, but a fresh-host restore drill
  remains unproven. The optional worker node and configured target snapshots
  are also not yet available.

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

## Follow-up: expired operator group and deploy queue (2026-11-07, 17:25 UTC)

- The failed transient cleanup was for the separate `leslieghf` membership in
  `vapor-temp-ssh`. That group still had a passwordless policy for the typed
  deploy and service-control wrappers. Root removed only `leslieghf` from that
  group with `gpasswd`; the account's normal login and `sudo` group membership
  remain. The shared group remains for temporary accounts and SSH matching; the
  failed transient service state was reset. This does not affect the independent
  root recovery key.
- Source review then found that the group-wide sudo rule superseded every
  request-specific capability file: any member could call both wrappers even
  if its grant allowed only one. No temporary grants were active, so root
  removed `/etc/sudoers.d/vapor-temporary-access` and verified the remaining
  sudoers configuration. The Platform installer now removes only the exact
  legacy rule and refuses an unexpected file; new grants rely on their
  per-request sudoers policy.
- The `vpsssh_240aab1d` account last ran read-only commands successfully at
  11:49:17 UTC; its SSH login was rejected as an invalid user 35 seconds later.
  The root-owned `/etc/passwd` timestamp still shows only the 11:06:20 UTC
  creation time. No auditd event is available, so the disappearance mechanism
  remains unknown rather than attributable to the group cleanup.
- Two old approved deployment attempts explain prior service failures. Request
  `bdf86cf7-f6b5-4681-bc9d-ee644d2ca5a3` reached the server but Platform failed
  to start because its SQLite audit-trigger migration tried to create an
  already-existing trigger; restart attempts then encountered a read-only
  filesystem. Request `7df36dcf-20cb-4d44-a027-09f48c4143f5` failed before
  deployment because it was a legacy request without an explicit
  `deployment_scope`; the worker reported a topology/argument mismatch. Neither
  request is currently active, and the Platform service now runs normally.
- The live queue currently contains four pending full-services requests for
  main revisions, including the latest `b1e2d3c`. The older requests are
  superseded by descendants and should not be approved as rollback targets.
  The live Platform revision remains `12192d8`; `b1e2d3c` contains later
  homepage, admin-card, and recovery-retention changes and is the current
  candidate for owner review. No deployment was queued or started during this
  inspection.
- GitHub Actions reports success for the current documentation branch through
  `a2b78b4` in CI run `42395734773`. This run succeeded. The deployment-status
  update in this section is documentation-only and was added after that run.

## Root recovery and deployment queue recheck (2026-11-07, 17:36 UTC)

- The owner-installed root SSH key now authenticates successfully. This was a
  read-only verification; no root login policy or key was changed by the agent.
- The stale temporary grant `240aab1d-8edc-49bd-bbd1-47706983e2c3` is recorded
  as revoked, its Linux account is absent, `vapor-temp-ssh` has no members, and
  `visudo -c` succeeds. The old broad group sudoers file was removed during
  the earlier root-assisted reconciliation. The review-branch source fix
  removes only that exact legacy file and now refuses installation if any
  group-wide sudo rule for `vapor-temp-ssh` remains elsewhere. It has not been
  deployed.
- At this check, the Platform checkout is detached at
  `12192d85348432ee24d1ce7c02ff2dab6eeded84`; Platform and all seven service
  units plus Caddy report active. The Platform process has been running since
  16:59 UTC. The release binary timestamp is 14:56 UTC, so this is still the
  previously deployed build, not the newer main candidate.
- Four full-deployment requests remain pending. The newest is
  `96b44a5a-c3c4-4d7c-a4ae-c05d9e5042f9` for `b1e2d3c`; the three older
  requests target ancestors and should not be selected as rollback revisions.
  This candidate still contains the prior `install-platform-ops.sh` behavior
  that recreates the broad `%vapor-temp-ssh` sudo rule. Do not approve or
  dispatch it until a newer candidate includes the per-grant installer fix.
  No approval or dispatch was performed in this check.
- Historical request
  `5338a3de-fd38-4c89-94ed-5afba23722ab` records a successful deployment in
  `result_detail` while retaining `dispatch_state=failed` and a generic helper
  failure in `dispatch_detail`. This is internally inconsistent historical
  reporting from the older deployed lifecycle; current source has explicit
  race handling for worker progress, but the live database row should remain
  historical rather than be rewritten to imply a verified dispatch outcome.
- The CI run for review commit `5d48163` was still in progress at 17:34 UTC.
  The per-grant sudo policy regression tests added after that commit have not
  yet run in Linux CI.

## Verification follow-up (2026-11-07, 17:47 UTC)

- The live `deploy/scripts/health-check.sh` completed successfully over the
  root SSH connection and reported all service-mode endpoint checks passed.
  It did not deploy or restart any service.
- CI run `42397727764` for review commit `31996ac` completed successfully,
  including the Linux Platform recipe and Windows recovery job. The Linux run
  compiled and executed the Unix-only per-grant sudo policy regression test.
- Local Windows checks also passed: `cargo test --locked` (99 tests across
  Platform/helper targets) and the deployment Python suite (91 tests, 10
  platform-specific skips). A local Linux cross-check could not run because
  this workstation has no `x86_64-linux-gnu-gcc`; the GitHub Linux CI job is
  the successful Linux evidence.

## Temporary SSH sudo-policy guard follow-up (2026-11-07, 18:11 UTC)

- Linux CI first caught that the new Unix-only regression test had not imported
  its private scanner helper; the import was fixed. The next run then exposed a
  real pattern bug: a direct `%vapor-temp-ssh` rule contains one percent sign,
  while the original expression accidentally required two. The scanner now
  catches both direct rules and aliases while ignoring ordinary comments.
- A further review found that sudoers accepts `#include` and `@include` paths
  outside `/etc/sudoers.d`. The installer and runtime helper now fail closed
  on alternate include paths so their group-rule scan covers the active policy
  files rather than silently missing an external include. The live host's
  `/etc/sudoers` uses only `@includedir /etc/sudoers.d`; the temporary group is
  empty, and the earlier `visudo -c` check passed.
- Linux CI run `42399846154` for `0824036` passed both the Platform recipe
  (including the Unix-only direct-rule, alias, comment, missing-path, and
  include-layout tests) and the Windows recovery tests. Local checks passed the
  deployment Python suite (91 tests, 10 skips), helper-target cargo tests
  (12 tests), installer Bash syntax, and `git diff --check`.
- These source changes remain on the review branch and have not been deployed.
  The live pending main candidate still contains the legacy installer behavior
  and must not be dispatched; no approval or deployment was performed.

## Effective temporary sudo scope check (2026-11-07, 18:24 UTC)

- The helper now asks sudo for the account's resolved command list after it
  writes a temporary grant, and again during `ssh verify`. It compares that
  effective list with the exact capability-derived commands. If another rule
  adds commands (including `ALL`), grant setup fails and rolls back; a mismatch
  also makes verification fail. The explicit `host_admin` capability remains
  intentionally unrestricted and is separately confirmed during grant setup.
- The parser regression test covers an exact two-command grant, an injected
  `ALL` command, a no-sudo account, and an unexpected command on a no-sudo
  grant. The VPS's actual sudo list output was checked read-only for the normal
  `vapor` account and matches the format being parsed; no account or policy was
  changed.
- CI run `42400917506` for `11840ff` passed both the Linux Platform recipe and
  Windows recovery tests. Local Windows helper tests (12), deployment Python
  tests (91, 10 platform skips), Bash syntax, and whitespace checks also pass.
- The implementation is still only on the review branch. The VPS remains on
  `12192d8`, and no pending deployment was approved or dispatched.

## Root SSH recovery and live deployment queue (2026-11-07, 18:31 UTC)

- The owner-installed break-glass root public key now authenticates from the
  workstation. A read-only status check reports host `ubuntu`, Platform
  `active`, deployed checkout `12192d8`, valid `visudo -c`, and an empty
  `vapor-temp-ssh` group. The live `health-check.sh` also passed in services
  mode. No service or host configuration was changed.
- The live deployment queue still contains four pending requests. The newest
  targets `b1e2d3c`; other pending commits include the currently deployed
  `12192d8` and older revisions. The newest request remains the candidate
  whose installer recreates the broad temporary-SSH sudo rule; it must not be
  approved or dispatched. The sudo hardening remains on the review branch and
  has not reached the VPS.
- The latest docs-only CI run `42401200728` completed successfully for both
  Linux and Windows. This records source/CI and host-health evidence only; no
  deployment request was approved, dispatched, or edited.

## Owner USB recovery task verified (2026-11-08, 00:00 UTC)

- The owner-formatted `E:` removable exFAT drive (`BACKUPS`) is mounted, with
  recovery data under `E:\VaporRecovery`. Its current root export is the
  2026-11-07 23:50:59 UTC Platform archive pinned to
  `739debf5c136badc8e65e3fd681bea63a8bca77c`; the archive checksum and the
  recovery-kit preflight record are present on the drive.
- A real scheduled run of `Vapor USB Recovery Sync` returned Task Scheduler
  result `0` at 01:00 local time / 00:00 UTC. The task is Ready and scheduled
  for its next hourly run. Its action uses the inbox Windows PowerShell 5.1
  path and invokes the user-local recovery copy with the root key by path; the
  private key is not copied to USB or printed.
- The run rehashed the current archive and one rotating historical archive.
  The report lists 29 retained archives and 139,135,191 bytes of archive
  payload, with 27 archives not verified during that run. This is scheduled
  off-host backup evidence, not a restore drill and not proof every historical
  archive is sound.
- The full Platform test recipe passed locally on Windows after the trigger
  fix: Atlas build, 105 Python tests (10 skipped), Platform/helper Rust tests,
  and all checked-out service Rust tests. Existing dead-code warnings remain.
  The scheduled-task helper test is also wired into the Windows CI job.
- No production VPS reset, Steam migration, clean-room purge, or fresh-host
  restore drill has been performed. The USB contents are plaintext and must be
  treated as sensitive because the recovery bundle contains server credentials.

## Emergency access and recovery workbench checkpoint (2026-11-08)

- Review branch `codex/platform-integration-20261102` is now at `2f19975`, nine
  commits ahead of `main` (`d6be2a1`); all source changes remain on the branch.
  The latest branch push includes the serialized SSH-audit schema migration,
  idempotent handling when the optional terminal unit is absent, and clearer
  archive-folder status in Recovery. Platform/helper `cargo check` and
  `git diff --check` passed after the latest change; GitHub CI is running for
  `2f19975`.
- The live VPS remains on `739debf5` in services mode. `/admin/recovery` is not
  deployed there yet. Both the public emergency-terminal route and its internal
  authorization callback return 404; the optional terminal service is not
  installed. No deployment ticket was created, approved, retried, or dispatched
  during this review.
- The read-only deployment-queue snapshot still has one pending request for
  `d6be2a1` and three older approved requests with failed dispatch state. None
  was modified or retried. The older worker error came from a request/worker
  topology mismatch; the relevant source fix preserves the request's pinned
  mode across host-config loading. The exact historical host config at that
  failure time remains unverified.
- The current branch's console remains opt-in and is not safe to enable on the
  shared public/admin origin. A public-service same-origin script flaw could
  issue privileged browser requests while a root Identity session is open.
  Keep it disabled until an admin origin/session boundary or verified Identity
  step-up exists. The installed live host has no terminal unit, and the public
  routes currently fail closed with 404.
- The latest local USB sync report is `2026-11-08T03:00:03Z`: it rehashed the
  latest archive and one rotating historical archive, found 31 retained
  archives, and left 29 historical archives unverified. It records successful
  local sync/preflight, not a clean-host restore drill or Platform-received USB
  evidence.
- Outstanding owner-level work: review and merge the nine-commit branch; decide
  when to open a deployment request for the current branch; isolate the admin
  origin or add a verifiable step-up before enabling browser root access; and
  complete a disposable fresh-host restore drill. Do not bulk-retry the failed
  deployment rows or label the platform fully recoverable until that drill is
  recorded.

## Root console dependency review at `1c19795` (2026-11-08)

- At this review point the branch was at `1c19795` and 12 commits ahead of
  `main` at `d6be2a1`. The preceding checkpoint's nine-commit count and
  `2f19975` CI reference are historical; the source does not include a
  deployment.
- The upstream ttyd repository still identifies `1.7.7` as its latest release.
  Its empty-WebSocket-frame crash report (`#1546`) and proposed fix (`#1574`)
  remain open. The reported crash requires a connected client; in this design,
  the client must first pass the root Identity and short-session checks, so
  this is an active-session availability risk. Because the process is a root
  shell, the optional installer now accepts only exact reviewed releases. No
  ttyd release is currently on that allowlist, so console preparation is
  intentionally blocked until a fixed release is reviewed.
- The root-terminal service remains disabled and absent on the live VPS. The
  branch's root-console route is not deployed or enabled. Root SSH/VNC remains
  the only live host recovery path.
- GitHub CI for `1c19795` and its parent `c79e4ed` completed successfully.
  This is source/CI evidence only; check the current head's CI before merging.
- The shared public/admin origin and lack of verified step-up remain separate
  blockers to enabling a browser root shell, even after ttyd becomes approved.
- Read-only production checks still return HTTP 200 for `/docs/` with the
  `Vapor Documentation` page, while `/admin/recovery` is 404 on the live
  revision as expected. The candidate separate origin
  `admin.vapor.ghf-studios.site` currently has no DNS record; no DNS, Caddy, or
  Identity settings were changed. Origin isolation therefore remains planned,
  not implemented.

## Root console version-parser review (2026-11-08)

- The runtime helper and optional installer now accept only the exact upstream
  `ttyd version <major>.<minor>.<patch>` output line. They do not search
  arbitrary output for a semver token, so a linked-library version cannot be
  mistaken for ttyd's version. The installer also requires the version command
  to exit successfully before considering its output. Unknown or additional
  output fails closed. Both approved-version lists remain empty.
- The parser was checked against upstream `src/server.c`, which prints
  `ttyd version %s` for `--version`. A read-only review found and prompted the
  installer exit-status correction; no other concrete parser bypass was
  identified.
- No ttyd release has been approved or installed, and no production deploy,
  service start, origin change, or Identity change was made. The emergency
  console remains unavailable on the live host and must remain disabled until
  the dependency, origin, and step-up gates are satisfied.
- Branch CI was green through `f88576f` before this parser patch. Recheck the
  current head's CI before merging; the documented compile and shell-syntax
  checks are not runtime validation.

## Emergency access continuation checkpoint (`359075d`, 2026-11-08)

- Branch `codex/platform-integration-20261102` is at `359075d`, 22 commits ahead
  of `main` at `d6be2a1`. Recent changes strictly parse ttyd's own version,
  reject nonzero version-command exits, keep its allowlist empty, prevent stale
  Approvals-page pagination responses from mutating a refreshed inbox, and
  make console session creation and its start audit one transaction. Backend
  start-failure and revocation transitions also write their audit event in the
  same transaction. The targeted console review found no concrete issue.
- Platform/helper `cargo check --locked --bin vapor_platform_server --bin
  vapor_platform_ops`, `git diff --check`, and Git Bash `bash -n` for the
  emergency-console installer passed. Local tests were not run. CI passed
  through `54b5e7b`; run `42446572188` for `359075d` was still in progress at
  this checkpoint.
- Read-only live-host inspection confirms Platform and Caddy are active, the
  VPS remains on `739debf`, and `/` has about 40 GB free. The emergency-console
  unit is inactive. `/docs/` returns 200 and `/docs` redirects to `/docs/`;
  `/admin/recovery` and `/admin/emergency-console` return 404 on the older live
  revision, as expected. No production files, services, DNS, or Identity
  settings were changed.
- The live deployment queue currently has one pending, unattempted request and
  three older approved requests whose dispatch state is failed. No deployment
  worker is active and no deployment timer is listed. These records were only
  read; none was retried or modified.
- The root-terminal feature is still not deployable for use: ttyd `1.7.7` is
  still the latest release, issue `#1546` remains open and proposed PR `#1574`
  remains unmerged. The exact-version allowlists remain empty. Separately, the
  shared public/admin origin and lack of verified Identity step-up still block
  activation. Recovery and console pages therefore remain source-only.

## Current live checkpoint (`fffbc779`, 2026-11-08 08:59 UTC)

- Platform revision `fffbc77981c2eb24d98f8a91a78ea67e7bf2e415` is deployed
  from successful request `18eb3672-fcb2-40b5-9751-f7d57c8c958a`. The request
  was approved through the installed root-operated queue command after both
  Linux Platform CI and Windows recovery tests passed. This is the current
  autonomous operator path; it does not add a separate assistant identity or
  change the provider trust boundaries.
- The eight independent services and Caddy are active. The deployed
  `health-check.sh` passed in services mode, `/healthz` and `/docs/` returned
  200, the state-export timer is enabled and active, and the VPS root filesystem
  has about 40 GB available. Build Cache is healthy but reports no configured
  current snapshots for either supported target.
- The recovery export at
  `2026-11-08T08:52:39Z` is pinned to this revision. The refreshed USB sync
  completed at `08:52:59Z`; its manifest names this revision, the archive hash
  matches, and every extracted recovery-kit checksum matches. The standalone
  validator from that extracted kit accepted the archive. The kit now includes
  `service_deploy_map.py`, fixing a missing import that made the previous kit's
  pre-package validator fail. The scheduled USB task remains Ready with result
  0 and its next hourly run is scheduled for 10:00 local time.
- This is structural/archive verification, not a fresh-host recovery drill.
  A disposable fresh host was not used for an end-to-end rebuild, so the
  platform is not yet proven fully recoverable. The active Vapor Client checkout also
  documents that `Vapor.lock` is not implemented; the worker/snapshot contract
  remains intentionally unconfigured until the canonical lock and build recipe
  exist. The primary-host local build path remains the working service-deploy
  path.
- Public docs routes are working at both `/docs` and `/docs/`. The emergency
  browser console remains unavailable and should stay disabled on the shared
  origin until its authentication/origin boundary is resolved.

## Current live checkpoint (`4078d1fb`, 2026-11-08 09:17 UTC)

- Platform composition `4078d1fb0caa7517df9f51c7beffb9c7e394a3f7` deployed
  successfully from request `370b0730-ec5a-4c95-bc45-e45d162a56b6` after the
  Linux and Windows CI jobs passed. Approval used the existing root-only typed
  queue command under the owner's explicit delegation; no new machine identity
  or auto-approve policy was introduced.
- The request result is `succeeded`; the full services-mode host health check
  passed, and public `/healthz` returned `ok`. The deployed-source inventory
  records all eight services plus Atlas at this composition revision.
- Hosting now reads Build Cache's bounded `/v1/status` response and reports
  configured/current counts separately from service health. Live Build Cache
  remains healthy with `0/2` snapshots current and `0/2` targets configured.
- The owner root-operator command remains the available autonomous approval
  path for an exact pending request. Previously failed dispatch tickets were
  left unchanged; only the matching, unexpired, CI-passed requests were
  approved.
- The earlier recovery, USB, fresh-host drill, and Vapor.lock/worker limitations
  remain as recorded in the preceding checkpoint; this deploy did not resolve
  them.

## Clean-destination USB recovery sync (`2026-11-09 00:18 UTC`)

- Ran the installed state-only USB sync into a newly created empty folder on the owner USB: `E:\VaporRecovery-CleanRebuild-20261109`. It created only `State-Snapshots/` and `Recovery-Kit/` at the destination root.
- The sync copied six indexed state generations (28,787,911 bytes) and pinned recovery kit `cbcf388536fdce8c18e852a8950fd6cabed5acee` separately from archived Platform source `20d1d234b6b2886250fc2ade4c3d016aebbdf708`.
- Independently checked every archive's length, SHA-256, sidecar, and state-bundle structure; all six passed. All nine recovery-kit files matched the pointer hashes. The latest archive hash is `da3c3d5da8a5bc36ff4779955e9df5b5be7ead61799a6e9f5ee4fc45b6035f24`.
- This proves a clean-destination USB sync and offline archive/kit validation. It does not prove a fresh Ubuntu host rebuild or state restore. Existing USB data was left intact; the requested bulk wipe was blocked by the command safety policy, so the clean test used a separate empty folder.

## Clean USB root and owner snapshot action (`2026-11-09`)

- With the owner's explicit authorization, reformatted only the verified `E:` USB volume (`BACKUPS`, Samsung Flash Drive FIT, exFAT, 119.5 GiB). The Windows recovery task completed with result `0` and recreated only `VaporRecovery/State-Snapshots/` and `VaporRecovery/Recovery-Kit/` at the USB root.
- The USB root contains six indexed state-only generations totaling 28,787,911 bytes and one pinned recovery-kit version. Every kit file is present; the latest archive's SHA-256 matches both its USB pointer and sidecar. The sync reuses the server's latest snapshot rather than creating a new server snapshot.
- Added an owner-only “Create snapshot now” action to the Recovery page. It requires a same-origin confirmation and Root Identity authorization, then asks the root helper to queue only the fixed `vapor-state-only-export.service`. The helper refuses if that unit is missing or already running. The page polls the verified archive status for up to 90 seconds and distinguishes a queued request from a confirmed published snapshot.
- `cargo check --bin vapor_platform_ops --bin vapor_platform_server` and `git diff --check` passed locally. Main-branch Linux and Windows CI both passed. Deployment request `2460bf74-a2fd-4536-8c71-04557690613e` completed successfully at `2026-11-09T00:47:09Z`; the deployed-source inventory records Platform composition `78722c2641b42bfdf493d45b6b92461dccd3e87f` for all nine targets. Public `/healthz` returned HTTP 200, the unauthenticated Recovery page returned the expected HTTP 401, and the Platform and service units are active. Four earlier pending requests for superseded commits were explicitly rejected as local-root decisions before the current revision was approved, preserving ordered deployment.
- No fresh-host restore drill has been performed; USB validation remains archive/kit validation only. The new snapshot button is live, but its authenticated browser flow still needs an owner-session check.

## Clean Ubuntu state-only recovery drill (`2026-11-09`)

- Rebuilt a disposable Ubuntu 24.04 x86_64 QEMU VM from the owner-held state bundle `vapor-platform-state-20261109T020509Z-2b9a10a5a222.tar.gz` (source pin `78722c2641b42bfdf493d45b6b92461dccd3e87f`). The VM used software emulation; production was not changed.
- The release build completed for the independent services, the state databases validated, and the state-only restore completed. Homepage `/`, docs `/docs/`, and the protected operator login route all returned HTTP 200 over strict local TLS. All nine core service/gateway units were active.
- Provider readiness intentionally remains incomplete in the disposable VM: it has a placeholder Steam API key and no GitHub OAuth credentials. The recovery marker remains `awaiting_provider_checks`; no success was claimed without real provider and owner-dashboard checks.
- The drill exposed three recovery issues: `.invalid` cannot satisfy strict Caddy TLS checks; resume validation rejected the intended root-owned `0640` service-group config files; and an export timer could collide with the restore process's state lock. Resume validation now accepts only root-owned `0600` files or `0640` files sharing the protected config directory's group. Recovery staging defers timer activation until the restore lock is released, then clears/retries a transient export failure from older snapshot revisions.
- After the restore lock was released, the state-only export completed successfully and published a validated 4.8 MB archive. The refreshed USB now contains 13 verified state generations (about 62.4 MB) and a recovery kit pinned to Platform `f9236de5ecbc5a1c2496c7a30cbf632b21fb815b`; the archived server source remains pinned independently to `78722c2641b42bfdf493d45b6b92461dccd3e87f`.
- Full deployment-script tests passed: 183 tests, 11 skipped. The VM still requires the real provider values and an owner sign-in/dashboard check before the recovery marker can be closed.
