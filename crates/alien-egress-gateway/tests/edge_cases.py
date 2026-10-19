"""Controlled network regressions for DNS failover, HTTP limits and timing metrics."""
import http.client
import json
import socket
import time


def check_edges(suite, audience, subject, proxy):
    assert suite.admin("/v1/policies", {
        "aud": audience, "sub": subject,
        "policy": {"allow": ["allowed.test", "failover.test", "mixed.test"]},
    })[0] == 204

    def request(host, fields):
        with socket.create_connection((proxy, 3128), timeout=8) as stream:
            headers = [f"Host: {host}", f"Proxy-Authorization: Bearer {suite.token()}"]
            headers.extend("X-Test: value" for _ in range(fields - len(headers)))
            stream.sendall((f"GET http://{host}/ HTTP/1.1\r\n" + "\r\n".join(headers) + "\r\n\r\n").encode())
            response = http.client.HTTPResponse(stream)
            response.begin()
            return response.status, response.read()

    def headers():
        response = request("allowed.test", 128)
        assert response == (200, b"controlled-origin-response\n"), response
        assert request("allowed.test", 129)[0] == 403

    def failover():
        # .250 has no endpoint on this bridge. Establish that the first address stalls.
        with socket.socket() as control:
            control.settimeout(1.5)
            start = time.monotonic()
            try:
                control.connect(("93.184.216.250", 80))
            except TimeoutError:
                assert time.monotonic() - start >= 1.4
            else:
                raise AssertionError("blackhole control did not time out")
        start = time.monotonic()
        assert request("failover.test", 2) == (200, b"controlled-origin-response\n")
        assert time.monotonic() - start < 2.5, "first address exhausted the dial deadline"
        # A good address must never be dialed when another answer is private.
        suite.tunnel(host="mixed.test", expected=403).close()

    def histograms():
        assert request("allowed.test", 2)[0] == 200
        status, metrics = suite.admin("/metrics", {}, method="GET")
        assert status == 200
        text = metrics.decode()
        for stage in ["dns", "connect"]:
            name = f"alien_egress_{stage}_duration_seconds"
            assert f"# TYPE {name} histogram\n" in text, text
            counts = [float(line.split()[-1]) for line in text.splitlines()
                      if line.startswith(name + "_bucket{")]
            assert counts and counts == sorted(counts) and counts[-1] >= 1, text
            count = float(next(line.split()[-1] for line in text.splitlines()
                               if line.startswith(name + "_count ")))
            assert count == counts[-1], text
        assert "host=" not in text and "sub=" not in text
        connection = http.client.HTTPConnection(proxy, 3129, timeout=2)
        try:
            connection.request("GET", "/metrics")
            assert connection.getresponse().status == 401
        finally:
            connection.close()

    failures = []
    for name, test in [("128 HTTP headers", headers), ("IPv4 blackhole failover", failover),
                       ("authenticated latency histograms", histograms)]:
        try:
            suite.record(name, test)
        except Exception as error:
            failures.append({"name": name, "error": str(error)})
    (suite.output / "edge-failures.json").write_text(json.dumps(failures, indent=2))
    assert not failures, failures
