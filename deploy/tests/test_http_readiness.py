from __future__ import annotations

import os
import pathlib
import subprocess
import tempfile
import unittest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
LIB = REPO_ROOT / "deploy" / "scripts" / "lib.sh"


class HttpReadinessTests(unittest.TestCase):
    def run_probe(self, *, attempts: int, fail_before: int) -> subprocess.CompletedProcess[str]:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = pathlib.Path(temporary.name)
        fake_bin = root / "bin"
        fake_bin.mkdir()
        call_count = root / "curl-count"
        calls = root / "calls"
        diagnostics = root / "diagnostics"

        (fake_bin / "curl").write_text(
            "#!/bin/bash\n"
            "url=\"${@: -1}\"\n"
            "printf '%s\\n' \"$*\" >> \"$READINESS_CALLS\"\n"
            "count=0; [ ! -f \"$READINESS_COUNT\" ] || count=$(cat \"$READINESS_COUNT\")\n"
            "count=$((count + 1)); printf '%s' \"$count\" > \"$READINESS_COUNT\"\n"
            "[ \"$count\" -gt \"$READINESS_FAIL_BEFORE\" ]\n"
        )
        (fake_bin / "sleep").write_text("#!/bin/sh\nexit 0\n")
        (fake_bin / "systemctl").write_text(
            "#!/bin/sh\nprintf 'systemctl %s\\n' \"$*\" >> \"$READINESS_DIAGNOSTICS\"\n"
        )
        (fake_bin / "journalctl").write_text(
            "#!/bin/sh\nprintf 'journalctl %s\\n' \"$*\" >> \"$READINESS_DIAGNOSTICS\"\n"
        )
        for executable in fake_bin.iterdir():
            executable.chmod(0o755)

        environment = os.environ.copy()
        environment.update(
            {
                "PATH": f"{fake_bin}:{environment['PATH']}",
                "VAPOR_CONFIG_DIR": str(root / "no-config"),
                "VAPOR_HEALTH_CHECK_ATTEMPTS": str(attempts),
                "VAPOR_HEALTH_CHECK_INTERVAL_SECONDS": "0",
                "READINESS_CALLS": str(calls),
                "READINESS_COUNT": str(call_count),
                "READINESS_DIAGNOSTICS": str(diagnostics),
                "READINESS_FAIL_BEFORE": str(fail_before),
            }
        )
        result = subprocess.run(
            [
                "bash",
                "-c",
                f"source {LIB!s}; wait_for_http 'test service' test.service http://127.0.0.1:7115/healthz --resolve test:443:127.0.0.1",
            ],
            check=False,
            env=environment,
            capture_output=True,
            text=True,
        )
        result.readiness_call_count = int(call_count.read_text()) if call_count.exists() else 0
        result.readiness_calls = calls.read_text() if calls.exists() else ""
        result.readiness_diagnostics = diagnostics.read_text() if diagnostics.exists() else ""
        return result

    def test_retries_service_startup_and_preserves_curl_options(self) -> None:
        result = self.run_probe(attempts=3, fail_before=2)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.readiness_call_count, 3)
        self.assertIn("--resolve test:443:127.0.0.1", result.readiness_calls)

    def test_reports_unit_status_and_journal_after_timeout(self) -> None:
        result = self.run_probe(attempts=2, fail_before=99)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.readiness_call_count, 2)
        self.assertIn("test service did not become ready after 2 attempts", result.stderr)
        self.assertIn("systemctl status --no-pager --full test.service", result.readiness_diagnostics)
        self.assertIn("journalctl --unit test.service", result.readiness_diagnostics)


if __name__ == "__main__":
    unittest.main()
