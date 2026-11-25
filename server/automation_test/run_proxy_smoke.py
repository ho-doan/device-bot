#!/usr/bin/env python3
"""Run the real Project/YAML proxy example against an isolated local fixture."""

import argparse
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile
import threading

from network_fault_fixture import start_server


ROOT = Path(__file__).resolve().parents[2]
EXAMPLE = ROOT / "server/examples/web-proxy-project"


def verify(directory):
    reports = list(directory.rglob("report.json"))
    if len(reports) != 1:
        raise RuntimeError(f"Expected one case report, found {len(reports)}")
    report_path = reports[0]
    report = json.loads(report_path.read_text(encoding="utf-8"))
    if report["status"] != "PASS":
        raise RuntimeError(f"Proxy test did not pass: {report.get('error')}")
    steps = {step["id"]: step for step in report["steps"]}
    ids = [
        "matched-http-503",
        "matched-latency",
        "matched-drop",
        "matched-websocket-reject",
        "matched-websocket-drop",
    ]
    hit_counts = {id: steps[id]["network_fault"]["active"]["hits"] for id in ids}
    if min(hit_counts.values()) < 1:
        raise RuntimeError(f"No proxy match for an expected fault: {hit_counts}")
    if steps["control-still-healthy"]["status"] != "passed":
        raise RuntimeError("The separate control browser did not stay healthy")
    if steps["clear-final-fault"]["network_fault"]["active"]["kind"] is not None:
        raise RuntimeError("The final fault was not cleared")
    timeline = [step["network_fault"]["elapsed_ms"] for step in report["steps"] if "network_fault" in step]
    if timeline != sorted(timeline):
        raise RuntimeError("Network fault evidence is out of time order")
    for screenshot in ["01-normal.png", "02-http-503.png", "03-recovered.png"]:
        if not (report_path.parent / screenshot).is_file():
            raise RuntimeError(f"Missing evidence screenshot {screenshot}")
    video = report_path.parent / "video.mp4"
    video_info = subprocess.check_output([
        "ffprobe", "-v", "error", "-show_entries", "format=duration:stream=codec_name",
        "-of", "json", str(video),
    ], text=True, timeout=20)
    media = json.loads(video_info)
    if float(media["format"]["duration"]) <= 0 or not any(
        stream.get("codec_name") == "h264" for stream in media["streams"]
    ):
        raise RuntimeError("Evidence recording does not contain playable H.264 video")
    print(json.dumps({"hits": hit_counts, "video": str(video),
                      "duration_seconds": float(media["format"]["duration"])}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    suffix = ".exe" if platform.system() == "Windows" else ""
    parser.add_argument("--device-bot", type=Path,
                        default=ROOT / f"server/target/debug/device-bot{suffix}")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    host = {"Darwin": "macos", "Windows": "windows"}.get(platform.system())
    if host is None:
        parser.error("this example requires a local macOS or Windows display")
    if not args.device_bot.is_file() or not shutil.which("ffprobe"):
        parser.error("build device-bot and install FFmpeg/ffprobe first")
    output = args.output or Path(tempfile.mkdtemp(prefix="device-bot-proxy-evidence-"))
    output.mkdir(parents=True, exist_ok=True)
    output = output.resolve()
    project = output / "fixture-project"
    shutil.copytree(EXAMPLE, project)
    manifest = project / "project.yaml"
    if host == "windows":
        manifest.write_text(manifest.read_text().replace("platform: macos", "platform: windows"))

    with start_server() as server:
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            address = f"127.0.0.1:{server.server_port}"
            env = os.environ | {
                "DEVICE_BOT_PROXY_API": address,
                "DEVICE_BOT_PROXY_URL": f"http://{address}/",
                "DEVICE_BOT_REPORT_DATA_DIR": str(output / "report-history"),
                "DEVICE_BOT_FLUTTER_AUTOSTART": "0",
                "DEVICE_BOT_IOS_AGENT_BIND": "127.0.0.1:0",
                "DEVICE_BOT_IOS_SIM_AGENT_BIND": "127.0.0.1:0",
            }
            for key in ("DEVICE_BOT_BIN", "DEVICE_BOT_CHROME_DEBUG_ADDRESS",
                        "DEVICE_BOT_CHROME_ACTIVE_PORT_FILE"):
                env.pop(key, None)
            subprocess.run([str(args.device_bot.resolve()), "run", str(project),
                            "--artifacts", str(output / "run"), "--rpc-timeout", "60",
                            "--case-timeout", "120"], env=env, check=True, timeout=180)
            verify(output / "run")
        finally:
            server.shutdown()
            thread.join()


if __name__ == "__main__":
    main()
