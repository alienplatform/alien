"""Controlled HTTP/TLS and DNS origin for docker_conformance.py; no external services."""
import http.server
import json
import socket
import ssl
import struct
import threading
from pathlib import Path


class Origin(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_GET(self):
        with open("/fixture/requests.jsonl", "a") as log:
            log.write(json.dumps({"path": self.path, "headers": dict(self.headers)}) + "\n")
        body = b"controlled-origin-response\n"
        if self.path == "/large":
            body = b"x" * (8 * 1024 * 1024)
        self.send_response(200)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(body)

    do_POST = do_GET

    def log_message(self, *args):
        pass


def serve(port, certificate=None):
    server = http.server.ThreadingHTTPServer(("0.0.0.0", port), Origin)
    if certificate:
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.load_cert_chain(f"/fixture/{certificate}.pem", f"/fixture/{certificate}.key")
        server.socket = context.wrap_socket(server.socket, server_side=True)
    server.serve_forever()


def dns():
    server = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    server.bind(("0.0.0.0", 53))
    while True:
        packet, sender = server.recvfrom(4096)
        offset, labels = 12, []
        while packet[offset]:
            length = packet[offset]
            labels.append(packet[offset + 1 : offset + 1 + length].decode())
            offset += length + 1
        offset += 1
        question = packet[12 : offset + 4]
        name = ".".join(labels)
        with open("/fixture/dns.jsonl", "a") as log:
            log.write(json.dumps({"name": name, "type": packet[offset : offset + 2].hex()}) + "\n")
        ips = ["93.184.216.2"]
        if name == "private.test":
            ips = ["10.77.0.2"]
        elif name == "metadata.test":
            ips = ["169.254.169.254"]
        elif name == "mixed.test":
            ips += ["10.77.0.2"]
        elif name == "rebind.test":
            ips = [Path("/fixture/rebind").read_text().strip()]
        if packet[offset : offset + 2] != b"\x00\x01":
            ips = []
        answer = packet[:2] + struct.pack("!HHHHH", 0x8180, 1, len(ips), 0, 0) + question
        for ip in ips:
            answer += b"\xc0\x0c" + struct.pack("!HHIH", 1, 1, 0, 4) + socket.inet_aton(ip)
        server.sendto(answer, sender)


for port, certificate in [(80, None), (443, "origin"), (9443, "origin"), (8443, "untrusted")]:
    threading.Thread(target=serve, args=(port, certificate), daemon=True).start()
dns()
