#!/usr/bin/env python3
"""Run the YAML self-tests against an isolated local backend and report store."""

import argparse
import base64
import json
import os
from pathlib import Path
import platform
import secrets
import socket
import subprocess
import time
import urllib.request


ROOT = Path(__file__).resolve().parents[2]
SUITE = Path(__file__).resolve().parent
CASES = {
    "qc-web": {}, "report-web": {}, "qc-desktop": {"desktop": True},
    "qc-import-pass": {"fixture": "pass.yaml", "payload": "qc_import_pass_payload", "runs": 2, "status": "PASS"},
    "qc-import-fail": {"fixture": "fail.yaml", "payload": "qc_import_fail_payload", "runs": 1, "status": "FAIL"},
    "qc-import-invalid": {"fixture": "invalid.yaml"},
    "qc-import-empty": {"fixture": "empty.yaml"},
    "qc-import-cancel": {"desktop": True},
    "qc-web-recovery": {},
    "report-empty-search": {}, "report-download": {"download": True},
    "report-persistence": {"restart": True},
}


def wait_backend(server, base_url):
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        if server.poll() is not None:
            raise RuntimeError("Backend exited; inspect backend.log")
        try:
            with urllib.request.urlopen(base_url + "/health/live", timeout=1) as response:
                if response.status == 200:
                    return
        except OSError:
            pass
        time.sleep(.1)
    raise RuntimeError("Backend did not become ready")


def arm_embedded_flow_shutdown_probe(base_url, token):
    headers = {"Content-Type": "application/json", "Authorization": "Bearer " + token}

    def post(body):
        request = urllib.request.Request(base_url + "/mcp", data=json.dumps(body).encode(), headers=headers)
        with urllib.request.urlopen(request, timeout=15) as response:
            if response.headers.get("mcp-session-id"):
                headers["mcp-session-id"] = response.headers["mcp-session-id"]
            data = response.read()
            return json.loads(data) if data else None

    post({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-11-25", "capabilities": {},
        "clientInfo": {"name": "shutdown-probe", "version": "1"}}})
    post({"jsonrpc": "2.0", "method": "notifications/initialized"})
    response = post({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {
        "name": "mobile_run_flow", "arguments": {
            "yaml": "version: 2\nname: shutdown-probe\nsteps:\n  - wait: {ms: 1}\n"}}})
    if response.get("result", {}).get("structuredContent", {}).get("status") != "PASS":
        raise RuntimeError("Embedded shutdown probe did not run")
    request = urllib.request.Request(base_url + "/mcp", headers=headers, method="DELETE")
    with urllib.request.urlopen(request, timeout=5):
        pass


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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--only", choices=CASES)
    parser.add_argument("--port", type=int, default=3338)
    parser.add_argument("--app", type=Path, help="Built QC app bundle/executable")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    host = {"Darwin": "macos", "Windows": "windows"}.get(platform.system())
    names = [args.only] if args.only else list(CASES)
    has_desktop = any(CASES[name].get("desktop") for name in names)
    if has_desktop and host is None:
        parser.error("desktop driver self-test requires macOS or Windows; use --only on Linux")
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", args.port))  # Refuse to reuse an unrelated backend.
    output = (args.output or ROOT / "artifacts" / "qc-self-test" /
              time.strftime("%Y%m%d-%H%M%S")).resolve()
    output.mkdir(parents=True, exist_ok=False)
    suffix = ".exe" if os.name == "nt" else ""
    binaries = ROOT / "server" / "target" / "debug"
    bot, runner, report = [binaries / (name + suffix)
                           for name in ("device-bot", "robot-flow", "robot-report")]
    for binary in (bot, runner, report):
        if not binary.is_file():
            parser.error("Build first: cd server && cargo build -p device-bot -p robot-flow -p robot-report")
    app = args.app
    if has_desktop:
        if app is None and host == "macos":
            products = ROOT / "server/flutter_app/build/macos/Build/Products/Debug"
            app = products / "mobile_mcp_companion_dev.app"
        if app is None or not app.exists():
            parser.error("Build QC desktop first, or supply --app PATH")
    env = os.environ.copy()
    env.update({
        "DEVICE_BOT_AUTH": secrets.token_hex(24),
        "QC_TEST_URL": f"http://127.0.0.1:{args.port}",
        "QC_TEST_APP": str(app.resolve()) if app else "",
        "DEVICE_BOT_FLUTTER_AUTOSTART": "0",
        "DEVICE_BOT_ARTIFACT_DIR": str(output / "backend-artifacts"),
        "DEVICE_BOT_RUNS_DIR": str(output / "runs"),
        "DEVICE_BOT_REPORT_DATA_DIR": str(output / "reports"),
        "DEVICE_BOT_QC_WEB_DIR": str(ROOT / "server/flutter_app/build/web"),
        "DEVICE_BOT_IOS_AGENT_BIND": "127.0.0.1:0",
        "DEVICE_BOT_IOS_SIM_AGENT_BIND": "127.0.0.1:0",
    })
    seed_report(report, output, env)
    results = []
    desktop = None
    desktop_log = None
    worker = None
    worker_log = None
    with (output / "backend.log").open("w") as log:
        server = subprocess.Popen([str(bot), "mcp", "--listen", f"127.0.0.1:{args.port}"],
                                  cwd=ROOT, env=env, stdout=log, stderr=log)
        try:
            wait_backend(server, env["QC_TEST_URL"])
            for name in names:
                config = CASES[name]
                if config.get("restart"):
                    arm_embedded_flow_shutdown_probe(env["QC_TEST_URL"], env["DEVICE_BOT_AUTH"])
                    server.terminate()
                    code = server.wait(timeout=15)
                    if os.name != "nt" and code != 0:
                        raise RuntimeError("Backend did not shut down cleanly after an embedded flow")
                    (output / "fixture").rename(output / "fixture-moved")
                    server = subprocess.Popen([str(bot), "mcp", "--listen", f"127.0.0.1:{args.port}"],
                                              cwd=ROOT, env=env, stdout=log, stderr=log)
                    wait_backend(server, env["QC_TEST_URL"])
                if config.get("desktop") and host == "macos" and desktop is None:
                    executable = app / "Contents" / "MacOS" / app.stem
                    desktop_log = (output / "qc-app.log").open("w")
                    desktop = subprocess.Popen([str(executable)], env=env,
                                               stdout=desktop_log, stderr=desktop_log)
                if config.get("fixture") or config.get("download"):
                    downloads = output / (name + "-downloads")
                    downloads.mkdir()
                    worker_log = (output / (name + "-chooser.log")).open("w")
                    fixture = SUITE / "fixtures" / config["fixture"] if config.get("fixture") else ""
                    worker = subprocess.Popen(["node", str(SUITE / "file_chooser.cjs"),
                                               str(output / "backend-artifacts/chrome-profile"),
                                               str(fixture), str(downloads)], stdout=worker_log, stderr=worker_log)
                command = [str(runner), "--mcp-url", env["QC_TEST_URL"] + "/mcp",
                           "--artifacts", str(output / name), "--case-timeout", "180"]
                command += ["--profiles", str(SUITE / "targets.yaml"),
                            "--target", host if config.get("desktop") else "web"]
                command.append(str(SUITE / (name + ".yaml")))
                with (output / (name + ".log")).open("w") as run_log:
                    completed = subprocess.run(command, cwd=ROOT, env=env, stdout=run_log,
                                               stderr=run_log, timeout=240)
                reports = sorted((output / name).glob("case-*/report.json"))
                data = json.loads(reports[-1].read_text()) if reports else {}
                ok = completed.returncode == 0 and data.get("status") == "PASS"
                result = {"test": name, "status": "PASS" if ok else "FAIL",
                          "report": str(reports[-1]) if reports else None,
                          "error": data.get("error")}
                if ok:
                    try:
                        result["historyEvidence"] = verify_history(env["QC_TEST_URL"], data["name"])
                    except (OSError, ValueError, KeyError, RuntimeError) as error:
                        result.update(status="FAIL", error=str(error))
                if ok and config.get("payload"):
                    try:
                        result["importedEvidence"] = verify_history(env["QC_TEST_URL"], config["payload"],
                                                                    config["status"], config["runs"])
                    except (OSError, ValueError, KeyError, RuntimeError) as error:
                        result.update(status="FAIL", error=str(error))
                if ok and config.get("download"):
                    deadline = time.monotonic() + 5
                    while time.monotonic() < deadline and not list(downloads.glob("*.json")):
                        time.sleep(.1)
                    files = list(downloads.glob("*.json"))
                    if not files or len(json.loads(files[0].read_text())["cases"]) != 2:
                        result.update(status="FAIL", error="Expected downloaded report JSON with two fixture cases")
                if worker is not None:
                    worker.terminate()
                    worker.wait(timeout=10)
                    worker = None
                    worker_log.close()
                    worker_log = None
                results.append(result)
                print(f"{result['status']}: {name}" + (f" — {result['error']}" if result["error"] else ""), flush=True)
        finally:
            if worker is not None:
                worker.terminate()
                worker.wait(timeout=10)
            if worker_log is not None:
                worker_log.close()
            if desktop is not None:
                desktop.terminate()
                try:
                    desktop.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    desktop.kill()
                    desktop.wait()
            if desktop_log is not None:
                desktop_log.close()
            server.terminate()
            try:
                server.wait(timeout=15)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait()
            (output / "summary.json").write_text(json.dumps(results, indent=2), encoding="utf-8")
    print(f"Evidence: {output}")
    return int(any(result["status"] != "PASS" for result in results))


if __name__ == "__main__":
    raise SystemExit(main())
