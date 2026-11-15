"""Exercise QC in disposable Chrome against an existing remote MCP backend.

Use DEVICE_BOT_AUTH for the remote bearer token. With --local-web, a loopback
proxy serves the locally built QC web assets and forwards MCP to that backend.
No installation or restart is performed on the remote machine.
"""
import argparse
import functools
import http.client
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import threading
import time
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parents[2]


class QcProxy(SimpleHTTPRequestHandler):
    def __init__(self, *args, remote, failures, observations, **kwargs):
        self.remote = remote
        self.failures = failures
        self.observations = observations
        super().__init__(*args, directory=str(ROOT / 'server/flutter_app/build/web'), **kwargs)

    def log_message(self, *_):
        pass

    def do_GET(self):
        if self.path.startswith('/qc/'):
            self.path = self.path[3:]
            return super().do_GET()
        self.send_error(404)

    def do_POST(self):
        self.forward()

    def do_DELETE(self):
        self.forward()

    def forward(self):
        if self.path != '/mcp':
            self.send_error(404)
            return
        body = self.rfile.read(int(self.headers.get('Content-Length', '0')))
        request = json.loads(body) if body else {}
        params = request.get('params', {})
        inspecting = (params.get('name') == 'mobile_list_elements_on_screen'
                      and params.get('arguments', {}).get('deviceId', '').startswith('windows:'))
        if inspecting and self.failures[0] > 0:
            self.failures[0] -= 1
            result = json.dumps({'jsonrpc': '2.0', 'id': request['id'], 'result': {
                'isError': True, 'content': [{'type': 'text',
                    'text': 'Windows Live: deadline has elapsed (test fault injection)'}]}}).encode()
            self.observations.append({'injected': True})
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(result)))
            self.end_headers()
            self.wfile.write(result)
            return
        connection = http.client.HTTPConnection(self.remote.hostname, self.remote.port, timeout=60)
        try:
            headers = {key: value for key, value in self.headers.items()
                       if key.lower() not in ('host', 'connection', 'content-length')}
            start = time.monotonic()
            connection.request(self.command, '/mcp', body, headers)
            response = connection.getresponse()
            payload = response.read()
            if inspecting:
                result = json.loads(payload).get('result', {})
                self.observations.append({'injected': False,
                    'seconds': round(time.monotonic() - start, 3),
                    'isError': result.get('isError', False),
                    'elements': len(result.get('structuredContent', {}).get('elements', []))})
            self.send_response(response.status)
            for key, value in response.getheaders():
                if key.lower() not in ('connection', 'transfer-encoding', 'content-length'):
                    self.send_header(key, value)
            self.send_header('Content-Length', str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)
        finally:
            connection.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--url', required=True)
    parser.add_argument('--scenario', type=Path, default=Path(__file__).with_name('qc-web.yaml'))
    parser.add_argument('--local-web', action='store_true')
    parser.add_argument('--inspection-failures', type=int, default=0)
    parser.add_argument('--repeat', type=int, default=1)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    remote = urlsplit(args.url)
    if (remote.scheme != 'http' or not remote.hostname or remote.username
            or remote.password or remote.path not in ('', '/') or remote.query or remote.fragment):
        parser.error('--url must be an http://host:port origin')
    if not os.environ.get('DEVICE_BOT_AUTH'):
        parser.error('set DEVICE_BOT_AUTH to the remote bearer token')
    if args.inspection_failures < 0 or (args.inspection_failures and not args.local_web):
        parser.error('--inspection-failures requires --local-web and a nonnegative count')
    if args.repeat < 1:
        parser.error('--repeat must be positive')
    if args.local_web and not (ROOT / 'server/flutter_app/build/web/main.dart.js').is_file():
        parser.error('build QC web assets first')
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    observations = []
    proxy = thread = None
    url = args.url.rstrip('/')
    try:
        if args.local_web:
            proxy = ThreadingHTTPServer(('127.0.0.1', 0), functools.partial(
                QcProxy, remote=remote, failures=[args.inspection_failures], observations=observations))
            thread = threading.Thread(target=proxy.serve_forever, daemon=True)
            thread.start()
            url = f'http://127.0.0.1:{proxy.server_port}'
        env = os.environ | {'QC_TEST_URL': url, 'DEVICE_BOT_FLUTTER_AUTOSTART': '0',
            'DEVICE_BOT_IOS_AGENT_BIND': '127.0.0.1:0',
            'DEVICE_BOT_IOS_SIM_AGENT_BIND': '127.0.0.1:0',
            'DEVICE_BOT_ARTIFACT_DIR': str(output / 'driver'),
            'DEVICE_BOT_REPORT_DATA_DIR': str(output / 'reports'),
            'DEVICE_BOT_RUNS_DIR': str(output / 'runs')}
        suffix = '.exe' if os.name == 'nt' else ''
        binaries = ROOT / 'server/target/debug'
        command = [str(binaries / ('robot-flow' + suffix)), '--server',
            str(binaries / ('device-bot' + suffix)), '--server-arg', 'mcp',
            '--profiles', str(Path(__file__).with_name('targets.yaml')), '--target', 'web',
            '--artifacts', str(output / 'case'), '--case-timeout', '240']
        command += [str(args.scenario.resolve())] * args.repeat
        with (output / 'runner.log').open('w') as log:
            result = subprocess.run(command, cwd=ROOT, env=env, stdout=log, stderr=log,
                                    timeout=300 * args.repeat)
        reports = [json.loads(path.read_text())
                   for path in sorted((output / 'case').glob('case-*/report.json'))]
        passed = (result.returncode == 0 and len(reports) == args.repeat
                  and all(report.get('status') == 'PASS' for report in reports))
        summary = {'status': 'PASS' if passed else 'FAIL', 'remote': args.url,
            'localWeb': args.local_web,
            'cases': [{'status': report.get('status'), 'error': report.get('error'),
                       'steps': len(report.get('steps', []))} for report in reports],
            'inspectionObservations': observations}
        (output / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
        print(json.dumps(summary, indent=2))
        return 0 if passed else 1
    finally:
        if proxy:
            proxy.shutdown()
            proxy.server_close()
            thread.join(timeout=5)


if __name__ == '__main__':
    raise SystemExit(main())
