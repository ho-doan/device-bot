"""Run real WebRTC/decode/input checks in disposable Chrome, primary or fallback."""
import argparse
import functools
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import socket
import subprocess
import threading
import time

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--mode', choices=['native', 'fallback'], required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    page = output / 'index.html'
    page.write_text("""<!doctype html><html><head><title>Device Bot Live Web Fixture</title>
<style>body{font:24px system-ui;margin:32px}input{height:48px;width:400px}
#pulse{width:500px;height:260px;margin-top:30px;background:blue}</style></head>
<body><label for="probe">Live input</label><input id="probe"><div id="pulse"></div>
<script>let tick=0;setInterval(()=>{document.querySelector('#pulse').style.background=
++tick%2?'blue':'orange'},200)</script></body></html>""")
    host = '127.0.0.1'
    if args.mode == 'fallback':
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as route:
            route.connect(('192.0.2.1', 9))
            host = route.getsockname()[0]
        if host.startswith('127.'):
            raise RuntimeError('fallback test requires a LAN route')
    handler = functools.partial(SimpleHTTPRequestHandler, directory=str(output))
    server = ThreadingHTTPServer((host, 0), handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    chrome_path = os.environ.get('DEVICE_BOT_CHROME_PATH',
        '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome')
    profile = output / 'chrome-profile'
    command = [chrome_path, '--headless=new', f'--user-data-dir={profile}',
               '--remote-debugging-address=127.0.0.1', '--remote-debugging-port=0',
               '--no-first-run', '--no-default-browser-check', 'about:blank']
    if args.mode == 'native':
        # Chromium's test-only switch accepts capture of this disposable tab.
        # No fake video source: getDisplayMedia + Region Capture must succeed.
        command.insert(-1, '--auto-accept-this-tab-capture')
    chrome = None
    diagnostics = None
    diagnostic_log = None
    try:
        with (output / 'chrome.log').open('w') as chrome_log:
            chrome = subprocess.Popen(command, stdout=chrome_log, stderr=chrome_log)
            active_port = profile / 'DevToolsActivePort'
            deadline = time.monotonic() + 20
            while not active_port.exists():
                if chrome.poll() is not None or time.monotonic() > deadline:
                    raise RuntimeError('disposable Chrome failed to start; see chrome.log')
                time.sleep(.1)
            endpoint = '127.0.0.1:' + active_port.read_text().splitlines()[0]
            diagnostic_log = (output / 'capture-stats.jsonl').open('w')
            diagnostics = subprocess.Popen(['node', str(ROOT / 'scripts/live_web_diagnostics.cjs'),
                                           endpoint], stdout=diagnostic_log, stderr=diagnostic_log)
            env = os.environ | {
                'DEVICE_BOT_LIVE_PLATFORM': 'web',
                'DEVICE_BOT_LIVE_DEVICE': 'chrome:browser',
                'DEVICE_BOT_LIVE_CHROME_ENDPOINT': endpoint,
                'DEVICE_BOT_LIVE_WEB_MODE': args.mode,
                'DEVICE_BOT_LIVE_ARTIFACTS': str(output / 'capture'),
                'DEVICE_BOT_LIVE_URL': f'http://{host}:{server.server_port}/index.html',
                'DEVICE_BOT_CHROME_DEBUG_ADDRESS': endpoint,
            }
            with (output / 'test.log').open('w') as log:
                result = subprocess.run(['cargo', 'test', '--manifest-path', 'server/Cargo.toml',
                    '-p', 'robot-stream', '--test', 'platforms', '--locked', '--',
                    '--ignored', '--nocapture'], cwd=ROOT, env=env,
                    stdout=log, stderr=log, timeout=300)
            (output / 'summary.json').write_text(json.dumps({
                'platform': 'web', 'mode': args.mode,
                'status': 'PASS' if result.returncode == 0 else 'FAIL',
                'testLog': str(output / 'test.log'),
            }, indent=2) + '\n')
            print((output / 'test.log').read_text())
            return result.returncode
    finally:
        if diagnostics is not None:
            diagnostics.terminate()
            diagnostics.wait(timeout=5)
        if diagnostic_log is not None:
            diagnostic_log.close()
        if chrome is not None:
            chrome.terminate()
            try:
                chrome.wait(timeout=10)
            except subprocess.TimeoutExpired:
                chrome.kill()
                chrome.wait()
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


if __name__ == '__main__':
    raise SystemExit(main())
