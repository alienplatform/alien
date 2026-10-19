"""Controlled HTTP/TLS and DNS origin for docker_conformance.py; no external services."""
import http.server
import json
import socket
import ssl
import struct
import threading
from pathlib import Path

# The fixture must accept the gateway's 128-field boundary; Python defaults to 100.
http.client._MAXHEADERS = 256


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


class MixedProtocolServer(http.server.ThreadingHTTPServer):
    """One port accepts real TLS, clear HTTP, and a binary echo protocol."""

    def finish_request(self, request, client_address):
        request.settimeout(8)
        first = request.recv(1, socket.MSG_PEEK)
        if first == b"\x00":
            echo_client(request)
            return
        if first == b"\x16":
            context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
            context.load_cert_chain("/fixture/origin.pem", "/fixture/origin.key")
            with context.wrap_socket(request, server_side=True) as tls:
                super().finish_request(tls, client_address)
            return
        super().finish_request(request, client_address)


def serve(port, certificate=None):
    server_type = MixedProtocolServer if port == 80 else http.server.ThreadingHTTPServer
    server = server_type(("0.0.0.0", port), Origin)
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
        if name == "slow.example.test":
            continue
        ips = ["93.184.216.2"]
        if name == "replicas.test":
            mode_file = Path("/fixture/peer-dns")
            mode = mode_file.read_text().strip() if mode_file.exists() else "normal"
            if mode in ["nxdomain", "nodata", "bare-nxdomain", "servfail", "refused"]:
                code = {"nxdomain": 3, "nodata": 0, "bare-nxdomain": 3, "servfail": 2, "refused": 5}[mode]
                authoritative = mode in ["nxdomain", "nodata"]
                answer = packet[:2] + struct.pack("!HHHHH", 0x8180 | code | (0x400 if authoritative else 0), 1, 0, int(authoritative), 0) + question
                if authoritative:
                    soa = b"\x02ns\x04test\x00\x0ahostmaster\x04test\x00" + struct.pack("!IIIII", 1, 1, 1, 1, 0)
                    answer += b"\xc0\x0c" + struct.pack("!HHIH", 6, 1, 0, len(soa)) + soa
                server.sendto(answer, sender)
                continue
            ips = ["93.184.216.3", "93.184.216.4"]
            if mode == "three":
                ips.append("93.184.216.5")
        elif name == "private.test":
            ips = ["10.77.91.2"]
        elif name == "configured-range.test":
            ips = ["93.184.217.2"]
        elif name == "overlay.test":
            ips = ["10.244.0.2"]
        elif name == "metadata.test":
            ips = ["169.254.169.254"]
        elif name == "mixed.test":
            ips += ["10.77.91.2"]
        elif name == "rebind.test":
            ips = [Path("/fixture/rebind").read_text().strip()]
        elif name == "failover.test":
            ips = ["93.184.216.250", "93.184.216.2"]
        if packet[offset : offset + 2] != b"\x00\x01":
            ips = []
        answer = packet[:2] + struct.pack("!HHHHH", 0x8180, 1, len(ips), 0, 0) + question
        for ip in ips:
            answer += b"\xc0\x0c" + struct.pack("!HHIH", 1, 1, 0, 4) + socket.inet_aton(ip)
        server.sendto(answer, sender)


def echo():
    server = socket.socket()
    server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    server.bind(("0.0.0.0", 2222))
    server.listen()
    while True:
        stream, _ = server.accept()
        threading.Thread(target=echo_client, args=(stream,), daemon=True).start()


def echo_client(stream):
    with stream:
        stream.settimeout(180)
        try:
            while data := stream.recv(1024):
                stream.sendall(data)
        except OSError:
            pass


threading.Thread(target=echo, daemon=True).start()
for port, certificate in [(25, "origin"), (465, "origin"), (587, "origin"), (80, None), (443, "origin"), (9443, "origin"), (8443, "untrusted")]:
    threading.Thread(target=serve, args=(port, certificate), daemon=True).start()
dns()
