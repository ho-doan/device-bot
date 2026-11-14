"""Disposable HTTP page and passive event evidence for QC Live YAML tests."""
import json
import socket
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlsplit


class LiveFixture:
    def __init__(self, host):
        page = Path(__file__).with_name('fixtures').joinpath('live.html').read_bytes()
        self.events = []
        self.lock = threading.Lock()
        fixture = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def do_GET(self):
                if urlsplit(self.path).path != '/live':
                    self.send_error(404)
                    return
                self.send_response(200)
                self.send_header('Content-Type', 'text/html; charset=utf-8')
                self.send_header('Content-Length', str(len(page)))
                self.send_header('Cache-Control', 'no-store')
                self.end_headers()
                self.wfile.write(page)

            def do_POST(self):
                if self.path != '/events':
                    self.send_error(404)
                    return
                size = int(self.headers.get('Content-Length', 0))
                if not 0 < size <= 16384:
                    self.send_error(400)
                    return
                try:
                    event = json.loads(self.rfile.read(size))
                    if not isinstance(event, dict) or not isinstance(event.get('session'), str):
                        raise ValueError('Invalid event')
                except (ValueError, UnicodeError):
                    self.send_error(400)
                    return
                with fixture.lock:
                    fixture.events.append(event)
                self.send_response(204)
                self.end_headers()

        self.server = ThreadingHTTPServer(('0.0.0.0', 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.url = f'http://{host}:{self.server.server_port}/live'

    def __enter__(self):
        self.thread.start()
        return self

    def __exit__(self, *_):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=5)

    def observations(self, test):
        with self.lock:
            return [dict(event) for event in self.events if event.get('test') == test]


def lan_host():
    # UDP connect selects a local route without sending a packet. HTTP on a LAN
    # address exercises Live's CDP fallback without a screen-sharing chooser.
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as probe:
        probe.connect(('192.0.2.1', 9))
        host = probe.getsockname()[0]
    if host.startswith('127.') or host == '0.0.0.0':
        raise RuntimeError('QC Live tests need a non-loopback LAN address')
    return host
