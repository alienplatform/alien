"""Exercise the guest's CONNECT/TLS bytes against a real TLS listener."""

import concurrent.futures
import hashlib
import json
import os
import pathlib
import select
import socket
import ssl
import subprocess
import sys
import tempfile


def listener():
    sock = socket.socket()
    sock.bind(("127.0.0.1", 0))
    sock.listen()
    sock.settimeout(10)
    return sock


def headers(sock):
    data = b""
    while not data.endswith(b"\r\n\r\n"):
        byte = sock.recv(1)
        assert byte, "peer closed before sending headers"
        data += byte
        assert len(data) < 8192
    return data


def origin(server, certificate, key):
    names = []
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(certificate, key)
    context.set_servername_callback(lambda _socket, name, _context: names.append(name))
    with server.accept()[0] as connection:
        connection.settimeout(10)
        with context.wrap_socket(connection, server_side=True) as secure:
            request = headers(secure)
            secure.sendall(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello")
    return names, request


def proxy(server, origin_port):
    with server.accept()[0] as connection:
        connection.settimeout(10)
        request = headers(connection)
        assert request.startswith(b"CONNECT allowed.test:443 HTTP/1.1\r\n")
        with socket.create_connection(("127.0.0.1", origin_port), timeout=10) as upstream:
            connection.sendall(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            while True:
                ready, _, _ = select.select([connection, upstream], [], [], 10)
                assert ready, "relay timed out"
                for source in ready:
                    data = source.recv(65536)
                    if not data:
                        return
                    (upstream if source is connection else connection).sendall(data)


with tempfile.TemporaryDirectory(prefix="alien-conformance-tls-") as directory:
    certificate = pathlib.Path(directory) / "cert.pem"
    key = pathlib.Path(directory) / "key.pem"
    subprocess.run(
        ["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes",
         "-keyout", str(key), "-out", str(certificate), "-days", "1",
         "-subj", "/CN=allowed.test",
         "-addext", "subjectAltName=DNS:allowed.test,DNS:other.test"],
        check=True, capture_output=True, timeout=30,
    )
    # Both names are valid on the controlled endpoint's certificate. A TLS
    # verification failure must not masquerade as an SNI enforcement result.
    for name in ("allowed.test", "other.test"):
        with listener() as target, listener() as gateway:
            with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
                served = pool.submit(origin, target, certificate, key)
                relayed = pool.submit(proxy, gateway, target.getsockname()[1])
                env = dict(os.environ, SSL_CERT_FILE=str(certificate),
                           HTTPS_PROXY=f"http://127.0.0.1:{gateway.getsockname()[1]}")
                completed = subprocess.run(
                    [sys.executable, sys.argv[1], json.dumps({
                        "kind": "http", "url": "https://allowed.test/probe",
                        "useProxy": True, "serverName": name, "hostHeader": None,
                    })],
                    env=env, check=True, capture_output=True, timeout=15,
                )
                result = json.loads(completed.stdout)
                assert result == {"outcome": "response", "status": 200,
                                  "bodySha256": hashlib.sha256(b"hello").hexdigest(), "bytes": 5}
                names, request = served.result(timeout=10)
                relayed.result(timeout=10)
                assert names == [name], names
                assert b"\r\nHost: allowed.test:443\r\n" in request
