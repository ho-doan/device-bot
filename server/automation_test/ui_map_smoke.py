"""Exercise the real MCP UI map against a disposable Chrome and local HTML fixture.

Set DEVICE_BOT_CHROME_DEBUG_ADDRESS to a disposable browser, then run:
    python3 server/automation_test/ui_map_smoke.py server/target/debug/device-bot
"""
import argparse
import json
import os
from pathlib import Path
import queue
import subprocess
import tempfile
from threading import Thread
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import time


PAGES = {
    "/": """<h1>Map Home</h1>
        <a id="inbox" href="/inbox">Open Inbox</a>
        <a id="settings" href="/settings">Open Settings</a>
        <label>Search <input id="search" aria-label="Search"></label>
        <button id="delete" onclick="document.body.innerText='Unexpected delete'">Delete data</button>
        <script>document.querySelector('input').addEventListener('input', e => {
          if (e.target.value === 'qc-fixture-query') {
            window.history.pushState({}, '', '/search');
            document.querySelector('h1').textContent = 'Map Results';
          }
        });</script>""",
    "/inbox": """<h1>Map Inbox</h1><a id="thread" href="/thread">Open Thread</a>""",
    "/settings": """<h1>Map Settings</h1><a id="home" href="/">Back Home</a>""",
    "/thread": """<h1>Map Thread</h1><a id="back" href="/inbox">Back to Inbox</a>""",
}


class Fixture(BaseHTTPRequestHandler):
    def do_GET(self):
        body = PAGES.get(self.path)
        if body is None:
            self.send_error(404)
            return
        encoded = ("<!doctype html><meta charset=utf-8>" + body).encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)

    def log_message(self, fmt, *args):
        pass


class Session:
    def __init__(self, process):
        self.process = process
        self.lines = queue.Queue()
        self.sequence = 0
        Thread(target=self._read, daemon=True).start()

    def _read(self):
        for line in self.process.stdout:
            self.lines.put(json.loads(line))
        self.lines.put(None)

    def request(self, method, params=None, notify=False):
        message = {"jsonrpc": "2.0", "method": method, "params": params or {}}
        if not notify:
            self.sequence += 1
            message["id"] = self.sequence
        self.process.stdin.write(json.dumps(message) + "\n")
        self.process.stdin.flush()
        if notify:
            return None
        while True:
            reply = self.lines.get(timeout=80)
            if reply is None:
                raise RuntimeError("MCP process closed before replying")
            if reply.get("id") == self.sequence:
                if "error" in reply:
                    raise RuntimeError(reply["error"])
                return reply["result"]

    def tool(self, name, **arguments):
        result = self.request("tools/call", {"name": name, "arguments": arguments})
        if result.get("isError"):
            raise RuntimeError(result.get("content"))
        return result.get("structuredContent", {})


def exercise(server, binary, artifacts):
    report = artifacts / "reports"
    env = {
        **os.environ,
        "DEVICE_BOT_REPORT_DATA_DIR": str(report),
        "DEVICE_BOT_IOS_AGENT_BIND": "127.0.0.1:0",
        "DEVICE_BOT_IOS_SIM_AGENT_BIND": "127.0.0.1:0",
        "DEVICE_BOT_FLUTTER_AUTOSTART": "0",
    }
    with (artifacts / "mcp.stderr.log").open("w") as log:
        with subprocess.Popen(
            [str(binary), "mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=log, text=True, env=env,
        ) as process:
            try:
                client = Session(process)
                client.request("initialize", {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {"name": "ui-map-smoke", "version": "1"},
                })
                client.request("notifications/initialized", notify=True)
                client.tool("mobile_boot_device", deviceId="chrome:browser")
                devices = client.tool("mobile_list_available_devices")["devices"]
                device_id = next(d["id"] for d in devices if d["platform"] == "chrome"
                                 and d["state"] == "available")
                client.tool("mobile_open_url", deviceId=device_id,
                            url=f"http://127.0.0.1:{server.server_port}/")
                time.sleep(0.4)
                initial = client.tool(
                    "mobile_ui_map", action="start", deviceId=device_id,
                    allowedTaps=["inbox", "settings", "thread", "home", "back", "delete"],
                    inputs={"search": "qc-fixture-query"}, settleMs=200,
                )
                map_id = initial["mapId"]
                deadline = time.monotonic() + 90
                while True:
                    snapshot = client.tool("mobile_ui_map", action="status", mapId=map_id)
                    if snapshot["status"] != "running":
                        break
                    if time.monotonic() >= deadline:
                        client.tool("mobile_ui_map", action="stop", mapId=map_id)
                        raise TimeoutError("UI map did not complete")
                    time.sleep(0.3)
                by_title = {screen["title"]: screen for screen in snapshot["screens"]}
                assert {"Map Home", "Map Inbox", "Map Thread", "Map Settings", "Map Results"} <= set(by_title), (by_title.keys(), snapshot["notes"])
                blocked = next(c for c in by_title["Map Home"]["controls"] if c["resourceId"] == "delete")
                assert blocked["status"] == "skipped", blocked
                assert "qc-fixture-query" not in json.dumps(snapshot)
                route = client.tool("mobile_ui_map", action="route", mapId=map_id, to="Map Thread")
                assert route["status"] == "observed", route
                assert [step["label"] for step in route["steps"]] == ["Open Inbox", "Open Thread"], route
                image = client.tool("mobile_ui_map", action="screenshot", mapId=map_id,
                                    screenId=by_title["Map Thread"]["id"])
                assert image["base64"].startswith("iVBORw0KGgo"), "Expected PNG evidence"
                client.tool("mobile_ui_map", action="add_source_routes", mapId=map_id, sourceRoutes=[
                    {"title": "Source Only", "route": "/source-only", "sourcePath": "src/router.dart",
                     "line": 19, "status": "candidate", "steps": ["Open candidate menu"]},
                ])
                draft = client.tool("mobile_ui_map", action="route", mapId=map_id, to="/source-only")
                assert draft["status"] == "source_only" and not draft["steps"], draft
                assert client.tool("mobile_ui_map", action="list")["maps"][0]["mapId"] == map_id
                print(f"PASS {len(by_title)} screens, {len(snapshot['transitions'])} transitions: {report / 'ui-maps' / map_id}")
            finally:
                process.stdin.close()
                try:
                    process.wait(timeout=8)
                except subprocess.TimeoutExpired:
                    process.terminate()
                    process.wait(timeout=8)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary", type=Path)
    parser.add_argument("--artifacts", type=Path)
    args = parser.parse_args()
    if not os.environ.get("DEVICE_BOT_CHROME_DEBUG_ADDRESS"):
        parser.error("set DEVICE_BOT_CHROME_DEBUG_ADDRESS to a disposable Chrome")
    artifacts = (args.artifacts or Path(tempfile.mkdtemp(prefix="device-bot-ui-map-"))).resolve()
    artifacts.mkdir(parents=True, exist_ok=True)
    with ThreadingHTTPServer(("127.0.0.1", 0), Fixture) as server:
        Thread(target=server.serve_forever, daemon=True).start()
        try:
            exercise(server, args.binary.resolve(), artifacts)
        finally:
            server.shutdown()


if __name__ == "__main__":
    main()
