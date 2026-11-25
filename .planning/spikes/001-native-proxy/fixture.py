"""Disposable cleartext loopback API and fault relay. Not a production proxy."""
import argparse
import base64
import hashlib
import ipaddress
import itertools
import json
from pathlib import Path
import select
import socket
import socketserver
import threading
import time
from urllib.parse import parse_qs, urlsplit


def record(**fields):
    print(json.dumps(fields), flush=True)


def reply(stream, code, body=b"", content_type="text/plain"):
    status = {200: "OK", 400: "Bad Request", 403: "Forbidden", 503: "Unavailable"}[code]
    stream.sendall(
        f"HTTP/1.1 {code} {status}\r\nContent-Length: {len(body)}\r\n"
        f"Content-Type: {content_type}\r\nConnection: close\r\n\r\n".encode() + body
    )


class Handler(socketserver.StreamRequestHandler):
    def handle(self):
        self.request.settimeout(5)
        try:
            self.dispatch()
        except (TimeoutError, OSError):
            pass

    def request_headers(self):
        line = self.rfile.readline(8192).decode("ascii").strip()
        headers = {}
        while True:
            next_line = self.rfile.readline(8192).decode("ascii").strip()
            if not next_line:
                break
            key, _, value = next_line.partition(":")
            headers[key.lower()] = value.strip()
        return line.split(), headers

    def dispatch(self):
        line, headers = self.request_headers()
        if len(line) != 3:
            return
        verb, target, _ = line
        server = self.server
        path = urlsplit(target).path
        websocket = headers.get("upgrade", "").lower() == "websocket"
        if server.role == "upstream":
            record(event="upstream", path=path, websocket=websocket)
            if websocket:
                key = headers["sec-websocket-key"] + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
                accept = base64.b64encode(hashlib.sha1(key.encode()).digest()).decode()
                self.request.sendall(
                    f"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n"
                    f"Connection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n".encode()
                    + b"\x81\x07healthy"
                )
                self.request.recv(1024)
            else:
                reply(self.request, 200, b"healthy")
            return
        if server.role == "proxy" and server.front:
            if path == f"/{server.token}/":
                body = server.front.replace(b"SPIKE_TOKEN", server.token.encode())
                reply(self.request, 200, body, "text/html; charset=utf-8")
                return
            if path == f"/{server.token}/result":
                raw = parse_qs(urlsplit(target).query).get("result", ["{}"])[0]
                report = json.loads(raw)
                if isinstance(report, dict):
                    record(event="browser-result", report=report)
                reply(self.request, 200, b"recorded")
                return
        if server.role == "control" or (
            server.role == "proxy" and server.front and path.startswith(f"/{server.token}/")
        ):
            route = path.split("/")
            if len(route) != 3 or route[1] != server.token:
                reply(self.request, 403)
                return
            modes = {"pass", "503", "delay", "drop", "ws-reject", "ws-drop"}
            if route[2] not in modes:
                reply(self.request, 400)
                return
            server.state["mode"] = route[2]
            record(event="mode", mode=route[2])
            reply(self.request, 200, b"set")
            return

        format_ = "origin"
        origins = server.origins
        if verb == "CONNECT":
            if target not in origins:
                reply(self.request, 403)
                return
            record(event="connect", target=target)
            self.request.sendall(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            line, headers = self.request_headers()
            if len(line) != 3:
                return
            verb, target, _ = line
            path = urlsplit(target).path
            websocket = headers.get("upgrade", "").lower() == "websocket"
            format_ = "connect"
        elif urlsplit(target).netloc:
            format_ = "absolute"
        authority = urlsplit(target).netloc or headers.get("host")
        if verb != "GET" or authority not in origins or path not in {"/items", "/socket"}:
            reply(self.request, 403)
            return
        mode = server.state["mode"]
        record(event="proxy", path=path, mode=mode, format=format_, websocket=websocket)
        if (not websocket and mode == "503") or (websocket and mode == "ws-reject"):
            reply(self.request, 503, b"injected")
            return
        if not websocket and mode == "drop":
            return
        if not websocket and mode == "delay":
            time.sleep(1.4)
        if not websocket:
            with socket.create_connection(("127.0.0.1", server.api), 3) as upstream:
                upstream.sendall(f"GET {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n".encode())
                for chunk in iter(lambda: upstream.recv(65536), b""):
                    self.request.sendall(chunk)
            return
        with socket.create_connection(("127.0.0.1", server.api), 3) as upstream:
            fields = "".join(f"{key}: {value}\r\n" for key, value in headers.items())
            upstream.sendall(f"GET {path} HTTP/1.1\r\n{fields}\r\n".encode())
            if mode == "ws-drop":
                chunk = b""
                while b"\x81\x07healthy" not in chunk and len(chunk) < 8192:
                    part = upstream.recv(1024)
                    if not part:
                        break
                    chunk += part
                self.request.sendall(chunk)
                time.sleep(0.2)
                record(event="ws-cut", confirmed=b"\x81\x07healthy" in chunk)
                return
            while True:
                readers, _, _ = select.select([self.request, upstream], [], [], 5)
                if not readers:
                    return
                for reader in readers:
                    data = reader.recv(65536)
                    if not data:
                        return
                    (upstream if reader is self.request else self.request).sendall(data)


class Server(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True


class SplitServer(Server):
    # An address-specific socket with SO_REUSEADDR can shadow a wildcard
    # listener with the same port on macOS.
    allow_reuse_address = False


def main():
    parser = argparse.ArgumentParser()
    lan = parser.add_mutually_exclusive_group()
    lan.add_argument("--lan")
    lan.add_argument("--lan-front", metavar="IPV4",
                     help="Bind only the relay to this LAN address. Keep the API and control on host loopback.")
    parser.add_argument("--api", type=int, default=8080)
    parser.add_argument("--proxy", type=int, default=18765)
    parser.add_argument("--control", type=int, default=18766)
    parser.add_argument("--token", default="spike")
    parser.add_argument("--front-page", type=Path)
    args = parser.parse_args()
    state = {"mode": "pass"}
    if args.lan_front:
        server_type = SplitServer
        try:
            front = ipaddress.IPv4Address(args.lan_front)
        except ipaddress.AddressValueError:
            parser.error("--lan-front requires a specific LAN IPv4 address")
        if front.is_loopback or front.is_unspecified or front.is_multicast:
            parser.error("--lan-front requires a specific LAN IPv4 address")
        listeners = [
            ("127.0.0.1", "upstream", args.api),
            (str(front), "proxy", args.proxy),
            ("127.0.0.1", "control", args.control),
        ]
        origins = {f"{front}:{args.proxy}"}
    else:
        server_type = Server
        hosts = ["127.0.0.1"] + ([args.lan] if args.lan else [])
        origins = {f"{host}:{args.api}" for host in hosts} | {"127.0.0.1:8080"}
        if args.front_page:
            origins |= {f"{host}:{args.proxy}" for host in hosts}
        listeners = [
            (host, role, port) for host, (role, port) in itertools.product(
                hosts, [("upstream", args.api), ("proxy", args.proxy), ("control", args.control)]
            )
        ]
    servers = []
    try:
        for host, role, port in listeners:
            server = server_type((host, port), Handler)
            server.role, server.state, server.api = role, state, args.api
            server.token, server.origins = args.token, origins
            server.front = args.front_page.read_bytes() if args.front_page else None
            servers.append(server)
            threading.Thread(target=server.serve_forever, daemon=True).start()
        record(event="ready", api=args.api, proxy=args.proxy, control=args.control, listeners=listeners)
        threading.Event().wait()
    except KeyboardInterrupt:
        pass
    finally:
        for server in servers:
            server.shutdown()
            server.server_close()


if __name__ == "__main__":
    main()
