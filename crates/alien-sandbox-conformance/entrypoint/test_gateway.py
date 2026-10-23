"""Positive controls prevent HTTP refusal or UDP reachability from becoming a denial."""
import contextlib
import hashlib
import http.server
import socket
import threading
import unittest

import gateway
from test_metadata import server

BODY = b"controlled-gateway-origin\n"


@contextlib.contextmanager
def closes_http():
    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            self.close_connection = True
        def log_message(self, *_):
            pass
    listener = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=listener.serve_forever)
    thread.start()
    try:
        yield listener.server_port
    finally:
        listener.shutdown()
        listener.server_close()
        thread.join()


@contextlib.contextmanager
def udp_echo():
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.bind(("127.0.0.1", 0))
    sock.settimeout(.1)
    stopped = threading.Event()
    def serve():
        while not stopped.is_set():
            try:
                data, peer = sock.recvfrom(1024)
                sock.sendto(data, peer)
            except socket.timeout:
                pass
    thread = threading.Thread(target=serve)
    thread.start()
    try:
        yield sock.getsockname()[1]
    finally:
        stopped.set()
        thread.join()
        sock.close()


class GatewayTests(unittest.TestCase):
    def config(self, allowed, denied, udp):
        return {"allowed": f"http://127.0.0.1:{allowed}/probe",
                "denied": f"http://127.0.0.1:{denied}/probe",
                "direct": f"http://127.0.0.1:{denied}/probe",
                "udpHost": "127.0.0.1", "udpPort": udp,
                "bodySha256": hashlib.sha256(BODY).hexdigest()}

    def test_origin_forbidden_is_reachability_not_policy_denial(self):
        with server(200, BODY) as allowed, server(403, b"forbidden") as denied, udp_echo() as udp:
            config = self.config(allowed, denied, udp)
            report = gateway.collect(config)
        self.assertEqual(report["allowed"]["bodySha256"], config["bodySha256"])
        self.assertEqual(report["denied"]["status"], 403)
        with self.assertRaises(AssertionError):
            gateway.verify(report, config)

    def test_reachable_udp_fails_even_when_tcp_connections_are_closed(self):
        with server(200, BODY) as allowed, closes_http() as denied, udp_echo() as udp:
            config = self.config(allowed, denied, udp)
            report = gateway.collect(config)
        self.assertIn("transportError", report["denied"])
        self.assertTrue(report["udp"]["received"])
        self.assertTrue(report["udp"]["echoMatched"])
        with self.assertRaises(AssertionError):
            gateway.verify(report, config)

    def test_changed_control_body_is_not_a_pass(self):
        with server(200, b"wrong fixture") as port:
            report = gateway.http_probe(f"http://127.0.0.1:{port}/probe")
        self.assertNotEqual(report["bodySha256"], hashlib.sha256(BODY).hexdigest())
        with self.assertRaises(AssertionError):
            gateway.verify({"allowed": report}, {"bodySha256": hashlib.sha256(BODY).hexdigest()})


if __name__ == "__main__":
    unittest.main()
