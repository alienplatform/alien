"""Guest observations for gateway qualification; pair denials with provider-side logs."""
import hashlib
import http.client
import json
import os
from pathlib import Path
import socket
import time
import urllib.error
import urllib.parse
import urllib.request


def http_probe(url):
    # No explicit proxy: these requests must exercise transparent routing for this UID.
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    try:
        try:
            response = opener.open(url, timeout=5)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            body = response.read(65537)
            return {"status": response.status, "bytes": len(body),
                    "bodySha256": hashlib.sha256(body).hexdigest()}
    except (OSError, http.client.HTTPException) as error:
        cause = error.reason if isinstance(error, urllib.error.URLError) else error
        return {"transportError": type(cause).__name__, "errno": getattr(cause, "errno", None)}


def udp_probe(host, port):
    payload = b"alien-entrypoint-control:" + os.urandom(16)
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
            sock.settimeout(5)
            sock.sendto(payload, (host, port))
            response, peer = sock.recvfrom(1024)
            return {"received": True, "echoMatched": response == payload,
                    "peer": list(peer), "bytes": len(response)}
    except OSError as error:
        return {"received": False, "transportError": type(error).__name__, "errno": error.errno}


def configuration():
    # Agent exec deliberately clears the image environment. Keep the same fixture
    # configuration available to boot, its children, and the first exec.
    encoded = os.environ.get("ALIEN_CONFORMANCE_GATEWAY")
    if not encoded:
        encoded = Path(__file__).with_name("gateway.json").read_text()
    return json.loads(encoded)


def collect(config):
    started = time.time()
    dns = {}
    for label in ("allowed", "denied"):
        host = urllib.parse.urlsplit(config[label]).hostname
        try:
            dns[label] = sorted({record[4][0] for record in socket.getaddrinfo(host, None, socket.AF_INET)})
        except OSError as error:
            dns[label] = {"error": type(error).__name__}
    report = {"allowed": http_probe(config["allowed"]),
            "denied": http_probe(config["denied"]),
            "direct": http_probe(config["direct"]),
            "udp": udp_probe(config["udpHost"], config["udpPort"]),
            "dns": dns,
            "requiresGatewayLogsAndPositiveControls": True}
    report.update(startedAt=started, completedAt=time.time())
    return report


def verify(report, config):
    assert report["allowed"].get("status") == 200, report
    assert report["allowed"]["bodySha256"] == config["bodySha256"], report
    for label in ("allowed", "denied"):
        assert isinstance(report["dns"][label], list) and report["dns"][label], report
    for label in ("denied", "direct"):
        # An origin's 403 is reachability, never proof of a network restriction.
        assert "transportError" in report[label] and "status" not in report[label], report
    assert report["udp"]["received"] is False, report
    # These observations alone do not prove policy enforcement. The live deployment
    # must separately pass public HTTP/UDP controls and correlate gateway denial logs.
    assert report["requiresGatewayLogsAndPositiveControls"] is True
