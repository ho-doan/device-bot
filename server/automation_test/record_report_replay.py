#!/usr/bin/env python3
"""Drive the actual macOS QC app, record report interactions, replay QC's YAML."""
import base64
import json
import os
from pathlib import Path
import re
import secrets
import socket
import subprocess
import sys
import time
import urllib.request

from run import ROOT, seed_report, wait_backend


class Mcp:
    def __init__(self, url, token):
        self.url, self.token, self.session, self.sequence = url, token, None, 0
        self.rpc("initialize", {"protocolVersion": "2025-11-25", "capabilities": {},
                 "clientInfo": {"name": "qc-record-replay", "version": "1"}})
        self.rpc("notifications/initialized", {}, notify=True)

    def rpc(self, method, params, notify=False):
        self.sequence += 1
        body = {"jsonrpc": "2.0", "method": method, "params": params}
        if not notify:
            body["id"] = self.sequence
        headers = {"Content-Type": "application/json", "Authorization": "Bearer " + self.token}
        if self.session:
            headers["mcp-session-id"] = self.session
        req = urllib.request.Request(self.url + "/mcp", data=json.dumps(body).encode(), headers=headers)
        with urllib.request.urlopen(req, timeout=90) as response:
            self.session = response.headers.get("mcp-session-id", self.session)
            raw = response.read()
        if not raw:
            return {}
        result = json.loads(raw)
        if "error" in result:
            raise RuntimeError(str(result["error"]))
        return result.get("result", {})

    def tool(self, name, **arguments):
        result = self.rpc("tools/call", {"name": name, "arguments": arguments})
        if getattr(self, "directory", None):
            (self.directory / f"mcp-{self.sequence:04d}-{name}.json").write_text(json.dumps(
                {"arguments": arguments, "result": result}, indent=2))
        if result.get("isError"):
            raise RuntimeError(str(result.get("content")))
        return result


class Qc:
    def __init__(self, client, directory):
        self.client, self.directory, self.sequence = client, directory, 0
        self.app = ROOT / "server/flutter_app/build/macos/Build/Products/Debug/mobile_mcp_companion_dev.app"

    def activate(self):
        self.client.tool("mobile_launch_app", deviceId="macos-host", appId=str(self.app))
        time.sleep(.3)

    def elements(self, text=None, kind=None, source="flutter"):
        self.activate()
        args = {"deviceId": "macos-host", "source": source, "maxResults": 100}
        if text:
            args["filter"] = text
        if kind:
            args["elementType"] = kind
        result = self.client.tool("mobile_list_elements_on_screen", **args)
        return result.get("structuredContent", {}).get("elements", [])

    def find(self, text, kind=None, timeout=30, source="flutter", predicate=None):
        deadline = time.monotonic() + timeout
        last = []
        while time.monotonic() < deadline:
            last = self.elements(text, kind, source)
            valid = [e for e in last if e.get("enabled") and
                     e["bounds"]["right"] > e["bounds"]["left"] and
                     e["bounds"]["bottom"] > e["bounds"]["top"] and
                     (predicate is None or predicate(e))]
            if valid:
                exact = [e for e in valid if text in (e.get("text"), e.get("contentDescription"))]
                return (exact or valid)[0]
            time.sleep(.15)
        (self.directory / "missing-element.json").write_text(json.dumps(last, indent=2))
        raise RuntimeError(f"QC control not found: {text}")

    def tap(self, element):
        b = element["bounds"]
        self.client.tool("mobile_click_on_screen_at_coordinates", deviceId="macos-host",
                         x=(b["left"] + b["right"]) / 2, y=(b["top"] + b["bottom"]) / 2)

    def click(self, text, **kwargs):
        self.tap(self.find(text, "button", **kwargs))

    def text(self, label, value):
        self.tap(self.find(label, "textField"))
        self.client.tool("mobile_type_keys", deviceId="macos-host", text="", clear=True)
        self.client.tool("mobile_type_keys", deviceId="macos-host", text=value)

    def screenshot(self, name):
        result = self.client.tool("mobile_take_screenshot", deviceId="macos-host")
        for item in result.get("content", []):
            if item.get("type") == "image":
                (self.directory / (name + ".png")).write_bytes(base64.b64decode(item["data"]))

    def console(self):
        """Keep one QC recording alive while examining its exposed UI controls."""
        for line in sys.stdin:
            try:
                command = json.loads(line)
                operation = command.pop("op")
                if operation == "done":
                    return
                if operation == "tool":
                    print(json.dumps({"ok": True, "result": self.client.tool(**command)}), flush=True)
                    continue
                if operation not in {"elements", "find", "click", "text", "tap", "screenshot"}:
                    raise ValueError("Unknown diagnostic operation")
                result = getattr(self, operation)(**command)
                print(json.dumps({"ok": True, "result": result}), flush=True)
            except Exception as error:
                print(json.dumps({"ok": False, "error": str(error)}), flush=True)


def main():
    with socket.socket() as probe:
        if probe.connect_ex(("127.0.0.1", 3338)) == 0:
            raise RuntimeError("Port 3338 is occupied; refusing to use an unrelated backend")
    output = ROOT / "artifacts/qc-record-replay" / time.strftime("%Y%m%d-%H%M%S")
    output.mkdir(parents=True)
    binaries = ROOT / "server/target/debug"
    app = ROOT / "server/flutter_app/build/macos/Build/Products/Debug/mobile_mcp_companion_dev.app"
    token = secrets.token_hex(24)
    url = "http://127.0.0.1:3338"
    # Use ordinary HTTP for the target page so Chrome Live exercises its CDP
    # fallback without a getDisplayMedia chooser covering the QC inspector.
    lan_ip = subprocess.check_output(["ipconfig", "getifaddr", "en0"], text=True).strip()
    report_url = f"http://{lan_ip}:3338/reports/"
    env = os.environ | {"DEVICE_BOT_AUTH": token, "QC_TEST_URL": url,
          "DEVICE_BOT_FLUTTER_AUTOSTART": "0", "DEVICE_BOT_ARTIFACT_DIR": str(output / "backend-artifacts"),
          "DEVICE_BOT_RUNS_DIR": str(output / "runs"), "DEVICE_BOT_REPORT_DATA_DIR": str(output / "reports"),
          "DEVICE_BOT_IOS_AGENT_BIND": "127.0.0.1:0", "DEVICE_BOT_IOS_SIM_AGENT_BIND": "127.0.0.1:0"}
    seed_report(binaries / "robot-report", output, env)
    server_log = (output / "backend.log").open("w")
    app_log = (output / "qc.log").open("w")
    server = subprocess.Popen([str(binaries / "device-bot"), "mcp", "--listen", "0.0.0.0:3338"],
                              env=env, stdout=server_log, stderr=server_log, cwd=ROOT)
    desktop = None
    qc = None
    try:
        wait_backend(server, url)
        client = Mcp(url, token)
        client.directory = output
        desktop = subprocess.Popen([str(app / "Contents/MacOS" / app.stem)],
                                   env=env, stdout=app_log, stderr=app_log)
        # Give AppKit time to register this process before requesting activation.
        # Activating the bundle too early can launch a second QC instance.
        time.sleep(3)
        client.tool("mobile_launch_app", deviceId="macos-host", appId=str(app))
        qc = Qc(client, output)
        qc.find("Devices (", timeout=60)
        qc.click("Start", predicate=lambda e: e.get("resourceId") == "device-chrome:browser")
        qc.click("Open URL on", timeout=90,
                 predicate=lambda e: "chrome:" in (e.get("resourceId") or ""))
        qc.text("Website URL", report_url)
        qc.screenshot("url-entered")
        (output / "url-ui.json").write_text(json.dumps(qc.elements(), indent=2))
        qc.click("Open URL")
        qc.click("Live control of", predicate=lambda e: "chrome:" in (e.get("resourceId") or ""))
        qc.find("Select an element", timeout=60)
        qc.screenshot("live-open")
        (output / "live-ui.json").write_text(json.dumps(qc.elements(), indent=2))
        print(f"Live ready. Evidence: {output}", flush=True)
        # Selection/recording actions are filled in after observing the real UI.
        if "--interactive" in sys.argv:
            qc.console()
    except Exception:
        if qc is not None:
            qc.screenshot("failure")
            (output / "failure-ui.json").write_text(json.dumps(qc.elements(), indent=2))
        raise
    finally:
        if desktop:
            desktop.terminate()
            try:
                desktop.wait(timeout=10)
            except subprocess.TimeoutExpired:
                desktop.kill(); desktop.wait()
        server.terminate()
        try:
            server.wait(timeout=15)
        except subprocess.TimeoutExpired:
            server.kill(); server.wait()
        server_log.close(); app_log.close()


if __name__ == "__main__":
    main()
