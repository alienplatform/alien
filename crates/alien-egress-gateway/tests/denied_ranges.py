"""Live destination-range revocation with unrelated traffic as a positive control."""
import http.client
import json
import socket
import ssl
import time
from pathlib import Path


def check_denied_range_refresh(suite, config, docker, containers, prefix, audience, origin, proxy):
    control = "93.184.216.6"
    name = prefix + "-range-control"
    docker("run", "-d", "--name", name, "--network", prefix, "--ip", control,
           "-v", f"{suite.output / 'origin'}:/fixture",
           "-v", f"{Path(__file__).with_name('origin.py')}:/origin.py:ro",
           "python:3.13-slim", "python", "/origin.py")
    containers.append(name)
    deadline = time.monotonic() + 10
    while True:
        try:
            socket.create_connection((control, 2222), timeout=1).close()
            break
        except OSError:
            assert time.monotonic() < deadline, "control origin did not start"
            time.sleep(.1)

    def echo(stream):
        stream.settimeout(1)
        stream.sendall(b"ping")
        data = b""
        while len(data) < 4:
            part = stream.recv(4 - len(data))
            if not part:
                raise ConnectionResetError("echo closed")
            data += part
        assert data == b"ping", data

    def proof():
        sub = "range-refresh"
        policy = {"allow": [f"{origin}:2222", f"{control}:2222", "allowed.test"]}
        assert suite.admin("/v1/policies", {"aud": audience, "sub": sub, "policy": policy})[0] == 204
        token = suite.token(sub=sub)
        addresses = [proxy, "93.184.216.4"]
        blocked, controls = [], []
        try:
            for address in addresses:
                stream = suite.tunnel(origin, 2222, token=token, proxy=address)
                blocked.append((stream, False))
                echo(stream)
                context = ssl.create_default_context(cafile=str(suite.output / "origin/ca.pem"))
                tls = context.wrap_socket(suite.tunnel(token=token, proxy=address), server_hostname="allowed.test")
                tls.sendall(b"GET / HTTP/1.1\r\nHost: allowed.test\r\nX-Keepalive: ")
                blocked.append((tls, True))
                stream = suite.tunnel(control, 2222, token=token, proxy=address)
                controls.append(stream)
                echo(stream)

            config["deniedRanges"].append(origin + "/32")
            secret_file = suite.output / "vault/secrets.json"
            replacement = secret_file.with_suffix(".next")
            replacement.write_text(json.dumps({"test": "fake-test-secret", "sandbox-egress-gateway-config": json.dumps(config)}))
            replacement.replace(secret_file)
            pending, closed = set(addresses), set()
            # Configuration polls every 60s and authorization sweeps every 30s.
            deadline = time.monotonic() + 100
            while pending or len(closed) < len(blocked):
                for stream in controls:
                    echo(stream)
                for address in list(pending):
                    conn = http.client.HTTPConnection(address, 3128, timeout=3)
                    conn.request("CONNECT", "allowed.test:443", headers={"Host": "allowed.test:443", "Proxy-Authorization": "Bearer " + token})
                    response = conn.getresponse()
                    assert response.status in [200, 403], response.status
                    if response.status == 403:
                        pending.remove(address)
                    conn.close()
                for index, (stream, tls) in enumerate(blocked):
                    if index in closed:
                        continue
                    try:
                        if tls:
                            stream.settimeout(.1)
                            stream.sendall(b"x")
                            try:
                                data = stream.recv(1)
                                assert not data, data
                                closed.add(index)
                            except socket.timeout:
                                pass
                        else:
                            echo(stream)
                    except (ConnectionError, ssl.SSLError):
                        closed.add(index)
                assert time.monotonic() < deadline, f"new dials pending={pending}; only {len(closed)}/4 existing tunnels closed"
                time.sleep(.2)
            for stream in controls:
                echo(stream)
        finally:
            for stream, _ in blocked:
                stream.close()
            for stream in controls:
                stream.close()
    suite.record("range refresh closes existing IP and named TLS tunnels on both replicas and preserves other IPs", proof)
