"""Fixture report setup and evidence checks for the automation suite."""

import base64
import json
import subprocess
import time
import urllib.request


def verify_report_download(downloads, timeout=10):
    """Wait for a complete fixture export, retaining evidence for missing/partial files."""
    deadline = time.monotonic() + timeout
    problem = "no completed JSON file"
    while time.monotonic() < deadline:
        for path in sorted(downloads.glob("*.json")):
            try:
                report = json.loads(path.read_text(encoding="utf-8"))
                cases = report.get("cases", [])
                names = {case["report"]["name"] for case in cases}
                if len(cases) == 2 and names == {"self-test passed", "self-test failure"}:
                    return {"file": path.name, "caseCount": len(cases)}
                problem = f"{path.name}: unexpected fixture cases {sorted(names)}"
            except (OSError, ValueError, KeyError, TypeError, AttributeError) as error:
                problem = f"{path.name}: {error}"
        time.sleep(.1)
    files = []
    for path in downloads.iterdir():
        try:
            if path.is_file():
                files.append({"name": path.name, "bytes": path.stat().st_size})
        except OSError:
            pass  # Chrome may rename a partial download while diagnostics are collected.
    raise RuntimeError(f"Expected downloaded report JSON with two fixture cases; {problem}; files={files}")


def verify_history(base_url, name, expected_status=None, expected_runs=1):
    def get(path):
        with urllib.request.urlopen(base_url + path, timeout=5) as response:
            return json.load(response)

    matching = {}
    for project in get("/reports/api/projects"):
        prefix = f"/reports/api/projects/{project['id']}/runs"
        for run in get(prefix):
            if run["name"] != name:
                continue
            detail = get(prefix + "/" + run["id"])
            for case in detail["cases"]:
                if case["report"]["name"] != name:
                    continue
                if expected_status and case["report"]["status"] != expected_status:
                    raise RuntimeError(f"Wrong history status for {name}")
                image = next((f for f in case["files"] if f["kind"] == "image"), None)
                if image is None:
                    raise RuntimeError(f"History for {name} has no image evidence")
                path = f"{prefix}/{run['id']}/cases/{case['id']}/files/{image['id']}"
                with urllib.request.urlopen(base_url + path, timeout=5) as response:
                    if not response.read().startswith(b"\x89PNG\r\n\x1a\n"):
                        raise RuntimeError("History evidence API did not return a PNG")
                matching[run["id"]] = path
    if len(matching) != expected_runs:
        raise RuntimeError(f"History for {name}: expected {expected_runs} runs, got {len(matching)}")
    return next(iter(matching.values()))


def seed_report(binary, directory, env):
    source = directory / "fixture"
    for label, status in [("passed", "PASS"), ("failure", "FAIL")]:
        case = source / label
        case.mkdir(parents=True)
        report = {
            "name": f"self-test {label}", "status": status,
            "selected_device": {"id": "fixture", "platform": "chrome"},
            "steps": [{"id": "fixture-step", "action": "assert_element",
                       "status": "passed" if status == "PASS" else "failed"}],
            "error": None if status == "PASS" else "Intentional fixture failure",
        }
        (case / "report.json").write_text(json.dumps(report), encoding="utf-8")
        (case / "fixture.png").write_bytes(base64.b64decode(
            "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII="))
    store = str(directory / "reports")
    project = json.loads(subprocess.check_output(
        [str(binary), "project-add", "--name", "QC self-test fixture",
         "--source", str(source), "--data-dir", store], env=env))
    subprocess.check_output(
        [str(binary), "publish", "--project", project["id"], "--input", str(source),
         "--name", "QC fixture run", "--data-dir", store], env=env)
    subprocess.check_output(
        [str(binary), "publish", "--project", project["id"], "--input", str(source),
         "--name", "QC fixture run 2", "--data-dir", store], env=env)
