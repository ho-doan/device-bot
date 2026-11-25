import http.client
import socket
import threading
import time
import unittest

from network_fault_fixture import start_server


class NetworkFaultFixtureTests(unittest.TestCase):
    def setUp(self):
        self.server = start_server()
        self.thread = threading.Thread(
            target=lambda: self.server.serve_forever(poll_interval=0.05),
            daemon=True,
        )
        self.thread.start()
        self.addCleanup(self.server.server_close)
        self.addCleanup(self.thread.join)
        self.addCleanup(self.server.shutdown)
        self.assertEqual(self.server.server_address[0], "127.0.0.1")

    def get(self, path):
        with_connection = http.client.HTTPConnection(
            "127.0.0.1", self.server.server_port, timeout=2
        )
        try:
            with_connection.request("GET", path)
            response = with_connection.getresponse()
            return response.status, response.read()
        finally:
            with_connection.close()

    def test_http_response_slow_endpoint_and_transport_drop_are_distinct(self):
        self.assertEqual(self.get("/api/unavailable")[0], 503)
        since = time.monotonic()
        self.assertEqual(self.get("/api/slow"), (200, b'{"ok":true}'))
        self.assertGreaterEqual(time.monotonic() - since, 0.5)
        with self.assertRaises((http.client.RemoteDisconnected, ConnectionResetError)):
            self.get("/api/drop")
        self.assertEqual(self.get("/")[0], 200)

    def test_websocket_can_reject_or_complete_handshake_then_close(self):
        self.assertEqual(self.get("/ws/reject")[0], 503)
        self.assertEqual(self.get("/ws/close")[0], 400)
        with socket.create_connection(self.server.server_address, timeout=2) as connection:
            connection.sendall(
                b"GET /ws/close HTTP/1.1\r\nHost: localhost\r\n"
                b"Connection: Upgrade\r\nUpgrade: websocket\r\n"
                b"Sec-WebSocket-Version: 13\r\n"
                b"Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n"
            )
            with connection.makefile("rb") as response:
                self.assertIn(b"101 Switching Protocols", response.readline())
                headers = {}
                for line in iter(response.readline, b"\r\n"):
                    if not line:
                        self.fail("The handshake ended before its headers")
                    key, value = line.split(b":", 1)
                    headers[key.lower()] = value.strip()
                self.assertEqual(
                    headers[b"sec-websocket-accept"], b"s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
                )
                self.assertEqual(response.read(4), b"\x88\x02\x03\xf3")


if __name__ == "__main__":
    unittest.main()
