import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPTS = Path(__file__).resolve().parent


class SourceInventoryTests(unittest.TestCase):
    def test_versions_transitives_and_scoped_packages(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "Cargo.lock").write_text(
                '[[package]]\nname="rust-dep"\nversion="1.2.3"\nsource="registry+fixture"\n'
            )
            (root / "studio").mkdir()
            (root / "studio/package-lock.json").write_text(json.dumps({"packages": {
                "": {"name": "project", "version": "1"},
                "node_modules/parent/node_modules/@scope/child": {"version": "2.4.6", "dev": True},
            }}))
            packages = [{"Name": "rust-dep", "Version": "1.2.3"},
                        {"Name": "@scope/child", "Version": "2.4.6"}]
            for case, contents, succeeds in [
                ("complete", packages, True),
                ("missing transitive", packages[:1], False),
                ("wrong version", [packages[0], {"Name": "@scope/child", "Version": "2.4.5"}], False),
                ("empty", [], False),
            ]:
                with self.subTest(case=case):
                    (root / "report.json").write_text(json.dumps({"Results": [{"Packages": contents}]}))
                    result = subprocess.run(["python3", str(SCRIPTS / "check-source-inventory.py"),
                                             "report.json"], cwd=root, capture_output=True)
                    self.assertEqual(result.returncode == 0, succeeds, result.stderr)


class ImageGateTests(unittest.TestCase):
    def test_actual_script_fails_closed_and_retains_reports(self):
        # Stub only external tools; execute the production inventory and CVE gate.
        packages = [{"Name": name} for name in
                    ["ffmpeg-9.0", "libass", "python-3.11", "nodejs-22", "torch",
                     "openai-whisper", "whisper-timestamped", "fastify"]]
        for case, inventory, findings, tool_failure, succeeds in [
            ("clean", packages, [], False, True),
            ("low", packages, [{"Severity": "LOW"}], False, True),
            ("unfixed high", packages, [{"Severity": "HIGH", "FixedVersion": ""}], False, False),
            ("critical", packages, [{"Severity": "CRITICAL"}], False, False),
            ("missing python", packages[:2] + packages[3:], [], False, False),
            ("missing ffmpeg", packages[1:], [], False, False),
            ("missing node", packages[:3] + packages[4:], [], False, False),
            ("empty", [], [], False, False),
            ("tool failure", packages, [], True, False),
        ]:
            with self.subTest(case=case), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                tools = root / "bin"
                tools.mkdir()
                (tools / "docker").write_text("#!/bin/sh\necho sha256:fixture\n")
                (root / "fixture.json").write_text(json.dumps({"Results": [{
                    "Packages": inventory, "Vulnerabilities": findings}]}))
                (tools / "trivy").write_text(
                    '#!/bin/bash\nset -eu\n'
                    'if [[ "$1" == image && "$FAIL_SCAN" == 1 ]]; then exit 2; fi\n'
                    'if [[ "$1" == version ]]; then echo "{}"; exit; fi\n'
                    'while [[ "$1" != --output ]]; do shift; done\n'
                    'cp "$FIXTURE" "$2"\n'
                )
                for tool in tools.iterdir():
                    tool.chmod(0o755)
                result = subprocess.run(["bash", str(SCRIPTS / "scan-image.sh")], cwd=root,
                    env={**os.environ, "PATH": f"{tools}:{os.environ['PATH']}",
                         "GITHUB_SHA": "fixture", "FIXTURE": str(root / "fixture.json"),
                         "FAIL_SCAN": str(int(tool_failure))}, capture_output=True)
                self.assertEqual(result.returncode == 0, succeeds, result.stderr)
                if not tool_failure:
                    self.assertTrue((root / "security-runtime/SHA256SUMS").is_file())
                    self.assertTrue((root / "security-runtime/sbom.spdx.json").is_file())
                    self.assertTrue((root / "security-runtime/sbom.cdx.json").is_file())


if __name__ == "__main__":
    unittest.main()
