# systemd

```text
vapor-homepage.service
vapor-docs.service
vapor-identity.service
vapor-diagnostics.service
vapor-deploy@.service
vapor-state-export.service
vapor-state-export.timer
vapor-state-only-export.service
vapor-state-only-export.timer
```

Each app service is a separate executable and systemd process running as the
`vapor` system user. The Platform process is only the shared admin gateway and
imports only `platform.env`. Service state is owned under `/var/lib/vapor-server`.

The deploy and state-export units run as root-owned oneshot operations. Each
approved deploy starts its own `vapor-deploy@<request-id>.service`; there is no
periodic deploy timer. The worker checks out and executes one exact
root-approved revision. Workers wait on the shared
`/run/vapor-server-deploy.lock` before claiming their request, so concurrent
approvals serialize instead of being rejected while still marked queued. If a
request expires while waiting, the claim step records it as expired and does
not deploy it. The deploy unit has no start timeout because a first build can
legitimately take a long time. A wedged worker therefore needs operator
attention before later deploy/export/restore work can acquire the shared lock.
Temporary SSH grants use a separate one-shot expiry timer per grant.

The state-only exporter writes under `/var/backups/vapor-server/state-only`
with its own pointer and retention. It snapshots the SQLite databases online
through SQLite's backup API, so hourly exports do not stop service writers.
The installer disables the legacy full-host timer; its retained archives are
read-only during migration and are never pruned by the new exporter.
