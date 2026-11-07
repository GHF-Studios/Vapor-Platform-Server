from __future__ import annotations

import json
import os
import pathlib
import subprocess
import sys
import tempfile
import textwrap
import unittest

from shell_test_support import bash_executable, posix_path


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
UPLOADER = REPO_ROOT / "deploy" / "scripts" / "upload-docs-via-ssh.sh"


class UploadDocsViaSshTests(unittest.TestCase):
    def test_stages_public_bundle_and_keeps_token_access_server_side(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            capture = root / "capture"
            capture.mkdir()
            bundle = root / "docs.tar.gz"
            bundle.write_bytes(b"public docs archive")
            fake_ssh = capture / "fake-ssh.py"
            fake_ssh.write_text(
                textwrap.dedent(
                    """\
                    #!/usr/bin/env python3
                    import json, os, pathlib, sys
                    args = sys.argv[1:]
                    capture = pathlib.Path(os.environ["MOCK_CAPTURE"])
                    with (capture / "ssh.jsonl").open("a") as stream:
                        stream.write(json.dumps(args) + "\\n")
                    command = args[-1]
                    if "mktemp -d /tmp/vapor-docs-upload." in command:
                        print("/tmp/vapor-docs-upload.ABC12345")
                    elif "sudo -v" in command:
                        marker = "printf '%s' '"
                        encoded = command.split(marker, 1)[1].split("' | base64 -d", 1)[0]
                        import base64
                        (capture / "remote.sh").write_bytes(base64.b64decode(encoded))
                    """
                ),
                encoding="utf-8",
                newline="\n",
            )
            fake_scp = capture / "fake-scp.py"
            fake_scp.write_text(
                "import json, os, pathlib, sys\n"
                "with (pathlib.Path(os.environ['MOCK_CAPTURE']) / 'scp.jsonl').open('a') as f:\n"
                "    f.write(json.dumps(sys.argv[1:]) + '\\n')\n",
                encoding="utf-8",
                newline="\n",
            )
            bash_env = root / "ssh-functions.sh"
            bash_env.write_text(
                'ssh() { "$PYTHON_FOR_TEST" "$MOCK_FAKE_SSH" "$@"; }\n'
                'scp() { "$PYTHON_FOR_TEST" "$MOCK_FAKE_SCP" "$@"; }\n',
                encoding="utf-8",
                newline="\n",
            )

            environment = os.environ.copy()
            environment["MOCK_CAPTURE"] = str(capture)
            environment["MOCK_FAKE_SSH"] = posix_path(fake_ssh)
            environment["MOCK_FAKE_SCP"] = posix_path(fake_scp)
            environment["PYTHON_FOR_TEST"] = posix_path(sys.executable)
            environment["BASH_ENV"] = posix_path(bash_env)
            result = subprocess.run(
                [
                    bash_executable(),
                    posix_path(UPLOADER),
                    "--bundle",
                    str(bundle),
                    "--host",
                    "example.test",
                    "--user",
                    "leslieghf",
                ],
                check=False,
                env=environment,
                capture_output=True,
                text=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr)

            ssh_calls = [json.loads(line) for line in (capture / "ssh.jsonl").read_text().splitlines()]
            scp_call = json.loads((capture / "scp.jsonl").read_text().splitlines()[0])
            remote_script = (capture / "remote.sh").read_text()
            self.assertGreaterEqual(len(ssh_calls), 3)  # stage, privileged POST, cleanup
            self.assertIn("/tmp/vapor-docs-upload.ABC12345/docs.tar.gz", scp_call[-1])
            self.assertIn("sudo -v", ssh_calls[1][-1])
            self.assertIn("sudo -n bash -s --", ssh_calls[1][-1])
            self.assertIn("/etc/vapor-server/docs.env", remote_script)
            self.assertIn("VAPOR_DOCS_ADMIN_TOKEN", remote_script)
            self.assertIn('Authorization: Bearer ${VAPOR_DOCS_ADMIN_TOKEN}', remote_script)
            self.assertNotIn("test-token", remote_script)
            self.assertIn("http://127.0.0.1:7112/current.tar.gz", remote_script)
            self.assertIn("rm -f", ssh_calls[-1][-1])


if __name__ == "__main__":
    unittest.main()
