# systemd

```text
vapor-homepage.service
vapor-docs.service
vapor-identity.service
vapor-diagnostics.service
vapor-deploy@.service
vapor-state-export.service
vapor-state-export.timer
```

The app services are installed separately and run as the `vapor` system user.
They read server-local env files from `/etc/vapor-server` and write state under
`/var/lib/vapor-server`.

The deploy and state-export units run as root-owned oneshot operations. Each
approved deploy starts its own `vapor-deploy@<request-id>.service`; there is no
periodic deploy timer. The worker checks out and executes one exact
root-approved revision. Temporary SSH grants use a separate one-shot expiry
timer per grant. Deploy/export/restore operations share
`/run/vapor-server-deploy.lock`, so they do not run concurrently.
