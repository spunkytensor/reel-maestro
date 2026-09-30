"""Reconcile resolved Rust registry and npm package/version pairs, including dev deps."""
import json
import sys
import tomllib
from pathlib import Path

report = json.loads(Path(sys.argv[1]).read_text())
packages = {
    (package["Name"], package["Version"])
    for result in report.get("Results", [])
    for package in result.get("Packages", [])
}
lock = tomllib.loads(Path("Cargo.lock").read_text())
expected = {
    (package["name"], package["version"])
    for package in lock["package"]
    if package.get("source", "").startswith("registry+")
}
node = json.loads(Path("studio/package-lock.json").read_text())
expected.update(
    (package.get("name", location.rsplit("node_modules/", 1)[-1]), package["version"])
    for location, package in node["packages"].items()
    if location and not package.get("link")
)
missing = expected - packages
if missing:
    sys.exit(f"Missing source packages: {sorted(missing)}")
print(f"Reconciled {len(expected)} Rust registry and Node package/version pairs")
