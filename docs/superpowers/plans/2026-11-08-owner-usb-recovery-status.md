# Owner USB Recovery Status Plan

**Design:** [`2026-11-08-owner-usb-recovery-status-design.md`](../specs/2026-11-08-owner-usb-recovery-status-design.md)

1. Add Python tests for strict report shape, timestamp/archive/hash validation,
   atomic replacement, and invalid-report preservation. Implement the fixed
   root-side recorder.
2. Add Rust tests and typed projection for the optional owner USB report;
   read it beneath the configured backup root with a strict size and symlink
   check. Malformed/missing reports remain unknown, not a failed VPS snapshot.
3. Extend the USB task to derive the latest archive digest from the verified
   index, send only the bounded summary over existing root SSH, and keep local
   sync successful if the report cannot be sent. Cover both outcomes in its
   PowerShell fixture tests.
4. Extend the Recovery page with recent/stale/unknown USB status and clear
   client-reported caveats. Keep the API root-authorized and private/no-store.
5. Run focused Python/PowerShell/Rust tests, then the full Platform test script.
   Review the diff for paths/secrets and push to `main`; wait for composed CI,
   queue the exact approved commit, export fresh state, sync the USB, and verify
   the dashboard/API and recovery kit.
