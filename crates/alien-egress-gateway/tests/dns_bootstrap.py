"""Three real gateway processes recover through empty authoritative discovery."""
import http.client
import json
import time


def check_dns_bootstrap(suite, start_gateway, docker, prefix, audience):
    replicas = [f"{prefix}-proxy{i}" for i in range(1, 4)]
    addresses = [f"93.184.216.{i}" for i in range(3, 6)]
    mode_file = suite.output / "origin/peer-dns"

    def status(address):
        conn = http.client.HTTPConnection(address, 3129, timeout=1)
        try:
            conn.request("GET", "/health")
            response = conn.getresponse()
            response.read()
            return response.status
        finally:
            conn.close()

    def await_status(expected):
        deadline = time.monotonic() + 12
        while True:
            try:
                observed = [status(address) for address in addresses]
                if observed == [expected] * 3:
                    return
            except (OSError, http.client.HTTPException):
                pass
            assert time.monotonic() < deadline, f"replicas did not reach {expected}"
            time.sleep(.1)

    def recover(mode):
        mode_file.write_text(mode)
        docker("kill", *replicas)
        docker("start", *replicas)
        await_status(200)
        token = suite.token(sub="dns-recovery")
        for address in addresses:
            suite.tunnel(token=token, proxy=address, expected=403).close()
        # Discovery becomes populated when these newly ready replicas are published.
        mode_file.write_text("three")
        record = suite.signed_policy({
            "aud": audience, "sub": "dns-recovery", "version": int(time.time()*1000),
            "policy": {"allow": ["allowed.test"]},
        })
        conn = http.client.HTTPConnection(addresses[0], 3128, timeout=5)
        conn.request("PUT", "/v1/policies/register", json.dumps({"record": record}), {
            "Proxy-Authorization": "Bearer " + token,
            "Content-Type": "application/json",
        })
        response = conn.getresponse()
        assert response.status == 204, (response.status, response.read())
        response.read()
        conn.close()
        for address in addresses:
            assert suite.tls(token=token, proxy=address) == (200, b"controlled-origin-response\n")

    mode_file.write_text("nxdomain")
    start_gateway(3)
    suite.record("three replicas recover after total loss with authoritative NXDOMAIN", lambda: recover("nxdomain"))
    suite.record("three replicas recover after total loss with authoritative NODATA", lambda: recover("nodata"))
    suite.record("configured service resolver negative answer needs no SOA record", lambda: recover("bare-nxdomain"))

    def failure(mode):
        mode_file.write_text(mode)
        docker("kill", *replicas)
        docker("start", *replicas)
        await_status(503)
        deadline = time.monotonic() + 2
        while time.monotonic() < deadline:
            assert [status(address) for address in addresses] == [503] * 3
            time.sleep(.1)
        mode_file.write_text("nxdomain")
        await_status(200)

    suite.record("SERVFAIL keeps all replicas unready until discovery recovers", lambda: failure("servfail"))
    suite.record("REFUSED keeps all replicas unready until discovery recovers", lambda: failure("refused"))
