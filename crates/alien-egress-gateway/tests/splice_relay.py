"""Real opaque/TLS transfers, half-close, accounting and Linux copy avoidance."""
import json
import re
import socket
import subprocess
import threading
import time


def check_splice(suite, audience, subject, proxy, origin, docker, prefix):
    assert suite.admin("/v1/policies", {
        "aud": audience, "sub": subject,
        "policy": {"allow": ["allowed.test", f"{origin}:2222"]},
    })[0] == 204
    container = prefix + "-proxy1"

    def large_tls():
        pid = docker("inspect", "--format", "{{.State.Pid}}", container)
        trace_path = suite.output / "transfer-syscalls.txt"
        # Requires host strace and permission to trace this owned container. -s 0 omits payloads.
        tracer = subprocess.Popen(["sudo", "-n", "strace", "-f", "-qq", "-s", "0",
                                   "-e", "trace=read,readv,recvfrom,recvmsg,write,writev,sendto,sendmsg,splice",
                                   "-o", str(trace_path), "-p", pid], stderr=subprocess.PIPE)
        try:
            time.sleep(.2)
            assert tracer.poll() is None, tracer.stderr.read().decode()
            started = time.monotonic()
            status, body = suite.tls(path="/large")
            elapsed = time.monotonic() - started
            assert status == 200 and body == b"x" * (8 * 1024 * 1024)
        finally:
            if tracer.poll() is None:
                subprocess.run(["sudo", "-n", "kill", "-INT", str(tracer.pid)], check=True)
            tracer.wait(timeout=5)
        copied, spliced = 0, 0
        for line in trace_path.read_text().splitlines():
            result = re.search(r"= (\d+)\s*$", line)
            if not result:
                continue
            count = int(result.group(1))
            if re.search(r"\bsplice(?:\(| resumed>)", line):
                spliced += count
            elif re.search(r"\b(?:read|readv|recvfrom|recvmsg)(?:\(| resumed>)", line):
                copied += count
        measurement = {"bytes": len(body), "seconds": elapsed,
                       "userspaceReadBytes": copied, "splicedBytes": spliced}
        (suite.output / "splice-transfer.json").write_text(json.dumps(measurement, indent=2))
        # Protocol inspection may copy bounded prefixes; bulk payload must stay in the kernel.
        assert copied < len(body) // 4 and spliced >= len(body), measurement

    def opaque():
        payload = bytes(range(256)) * 8192
        errors = []
        with socket.create_connection((proxy, 3128), timeout=15) as stream:
            # Pipeline a payload prefix with CONNECT to exercise already-inspected bytes.
            request = (f"CONNECT {origin}:2222 HTTP/1.1\r\nHost: {origin}:2222\r\n"
                       f"Proxy-Authorization: Bearer {suite.token()}\r\n\r\n").encode()
            stream.sendall(request + payload[:123])
            head = bytearray()
            while not head.endswith(b"\r\n\r\n"):
                data = stream.recv(1)
                assert data, head
                head.extend(data)
            assert head.startswith(b"HTTP/1.1 200"), head

            def send():
                try:
                    stream.sendall(payload[123:])
                    stream.shutdown(socket.SHUT_WR)
                except Exception as error:
                    errors.append(error)

            writer = threading.Thread(target=send)
            writer.start()
            # Delay reads briefly to exercise a full destination buffer.
            time.sleep(.1)
            received = bytearray()
            while data := stream.recv(8192):
                received.extend(data)
            writer.join(timeout=15)
            assert not writer.is_alive() and not errors, errors
            assert received == payload
        deadline = time.monotonic() + 3
        while True:
            records = []
            for line in docker("logs", container).splitlines():
                try:
                    fields = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if fields.get("event") == "close" and fields.get("port") == 2222:
                    records.append(fields)
            if records:
                record = records[-1]
                assert record["bytesUp"] == len(request) + len(payload), record
                assert record["bytesDown"] == len(head) + len(payload), record
                break
            assert time.monotonic() < deadline, "missing completed connection audit"
            time.sleep(.05)

    def descriptors():
        def count():
            return int(docker("exec", "-u", "65532", container, "sh", "-c",
                              "ls /proc/1/fd | wc -l"))
        before = count()
        for _ in range(32):
            with suite.tunnel(host=origin, port=2222) as stream:
                stream.sendall(b"descriptor-lifetime")
                stream.shutdown(socket.SHUT_WR)
                received = bytearray()
                while data := stream.recv(128):
                    received.extend(data)
                assert received == b"descriptor-lifetime"
        deadline = time.monotonic() + 3
        while count() > before + 4:
            assert time.monotonic() < deadline, "relay descriptors leaked across completed tunnels"
            time.sleep(.05)

    failures = []
    for name, test in [("TLS bulk transfer avoids userspace copies", large_tls),
                       ("opaque pipeline, backpressure, half-close and exact byte accounting", opaque),
                       ("completed tunnels release relay descriptors", descriptors)]:
        try:
            suite.record(name, test)
        except Exception as error:
            failures.append({"name": name, "error": str(error)})
    (suite.output / "splice-failures.json").write_text(json.dumps(failures, indent=2))
    suite.benchmark()
    assert not failures, failures
