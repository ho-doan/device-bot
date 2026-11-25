#!/usr/bin/env python3
"""Loopback-only browser/API fixture. It does not proxy or modify host networking."""

import argparse
import base64
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import socket
import struct
import time
from urllib.parse import urlsplit


PAGE = b"""<!doctype html>
<html lang="en"><meta charset="utf-8"><title>Device Bot network fixture</title>
<h1>Device Bot network fixture</h1>
<button id="trigger-status" onclick="http('/api/unavailable')">Request HTTP 503</button>
<button id="trigger-timeout" onclick="http('/api/slow', 150)">Request timeout</button>
<button id="trigger-drop" onclick="http('/api/drop')">Drop network connection</button>
<button id="trigger-ws-reject" onclick="ws('/ws/reject')">Reject WebSocket handshake</button>
<button id="trigger-ws-close" onclick="ws('/ws/close')">Close WebSocket after opening</button>
<button id="trigger-current" onclick="http('/api/current')">Read unchanged API</button>
<button id="trigger-short" onclick="http('/api/current', 150)">Read with a short deadline</button>
<button id="trigger-ws-stable" onclick="ws('/ws/stable')">Open stable WebSocket</button>
<p><output id="fault-result" aria-live="polite">Ready</output></p>
<script>
  const result = document.getElementById('fault-result');
  async function http(path, deadline) {
    result.textContent = 'Waiting for HTTP';
    try {
      const options = deadline ? {signal: AbortSignal.timeout(deadline)} : {};
      const response = await fetch(path, options);
      result.textContent = 'HTTP ' + response.status;
    } catch (error) {
      result.textContent = error.name === 'TimeoutError' ? 'Request timed out' : 'Network error';
    }
  }
  function ws(path) {
    result.textContent = 'Waiting for WebSocket';
    let opened = false;
    const socket = new WebSocket('ws://' + location.host + path);
    socket.onopen = () => { opened = true; result.textContent = 'WebSocket open'; };
    socket.onclose = (event) => {
      result.textContent = opened ? 'WebSocket closed ' + event.code : 'WebSocket handshake failed';
    };
  }
</script>
</html>
"""


class FaultHandler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, _format, *_args):
        # Do not record callers' URLs or any headers.
        return

    def _reply(self, status, body, content_type="application/json"):
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        try:
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            # The timeout demo deliberately aborts the download.
            self.close_connection = True

    def do_GET(self):
        path = urlsplit(self.path).path
        if path == "/":
            self._reply(200, PAGE, "text/html; charset=utf-8")
        elif path in {"/api/unavailable", "/ws/reject"}:
            self._reply(503, b'{"error":"fixture-unavailable"}')
        elif path == "/api/slow":
            time.sleep(0.6)
            self._reply(200, b'{"ok":true}')
        elif path == "/api/current":
            self._reply(200, b'{"source":"unchanged-api"}')
        elif path == "/api/drop":
            self.close_connection = True
            try:
                self.connection.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
        elif path in {"/ws/close", "/ws/stable"}:
            self._serve_websocket(hold=path == "/ws/stable")
        else:
            self._reply(404, b'{"error":"unknown-fixture-path"}')

    def _serve_websocket(self, hold):
        key = self.headers.get("Sec-WebSocket-Key", "")
        try:
            valid_key = len(base64.b64decode(key, validate=True)) == 16
        except ValueError:
            valid_key = False
        if self.headers.get("Upgrade", "").lower() != "websocket" or not valid_key:
            self._reply(400, b'{"error":"websocket-upgrade-required"}')
            return
        accept = base64.b64encode(
            hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()
        ).decode()
        self.send_response(101)
        self.send_header("Upgrade", "websocket")
        self.send_header("Connection", "Upgrade")
        self.send_header("Sec-WebSocket-Accept", accept)
        self.end_headers()
        if hold:
            self.connection.settimeout(30)
            try:
                self.rfile.read(1)
            except OSError:
                pass
        else:
            self.wfile.write(b"\x88\x02" + struct.pack("!H", 1011))
        self.close_connection = True


def start_server(port=0):
    return ThreadingHTTPServer(("127.0.0.1", port), FaultHandler)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", type=int, default=18781)
    args = parser.parse_args()
    with start_server(args.port) as server:
        print(f"http://127.0.0.1:{server.server_port}/", flush=True)
        try:
            server.serve_forever()
        except KeyboardInterrupt:
            pass


if __name__ == "__main__":
    main()
