"""One bounded network observation; invoked with a JSON request as argv[1]."""

import base64
import errno
import hashlib
import http.client
import ipaddress
import json
import os
import signal
import socket
import ssl
import struct
import sys
import urllib.parse

TIMEOUT = 5
MAX_BODY = 1024 * 1024

# Set once the first TCP connection completes. A deadline, reset or route error after
# that point means the network carried the traffic, so it cannot count as a denial.
CONNECTED = False


def deadline(_signum, _frame):
    raise TimeoutError()


def dns_query(probe):
    server = ipaddress.ip_address(probe["server"])
    labels = probe["host"].rstrip(".").encode("idna").split(b".")
    if any(not label or len(label) > 63 for label in labels):
        raise ValueError("invalid DNS label")
    name = b"".join(bytes([len(label)]) + label for label in labels) + b"\0"
    if len(name) > 255 or not 1 <= probe["port"] <= 65535:
        raise ValueError("invalid DNS name or port")
    question = name + struct.pack("!HH", 1, 1)
    transaction = os.urandom(2)
    request = transaction + struct.pack("!HHHHH", 0, 1, 0, 0, 0) + question
    family = socket.AF_INET6 if server.version == 6 else socket.AF_INET
    with socket.socket(family, socket.SOCK_DGRAM) as connection:
        connection.settimeout(TIMEOUT)
        # Connected UDP accepts replies only from the selected server and port.
        connection.connect((str(server), probe["port"]))
        connection.send(request)
        response = connection.recv(4096)
    if len(response) < 12:
        return {"outcome": "protocolFailed"}
    flags, questions = struct.unpack("!HH", response[2:6])
    if (response[:2] != transaction or not flags & 0x8000 or not flags & 0x0400
            or flags & 0x7800 or flags & 0x000F not in (0, 3)
            or questions != 1 or response[12:12 + len(question)] != question):
        return {"outcome": "protocolFailed"}
    # Even NXDOMAIN proves that the query reached a DNS server. Authoritative
    # query logs, including a positive control, establish exfiltration evidence.
    return {"outcome": "connected"}


def http_probe(probe):
    origin = urllib.parse.urlsplit(probe["url"])
    if origin.scheme not in ("http", "https") or not origin.hostname:
        raise ValueError("expected an HTTP(S) origin")
    if origin.username is not None or origin.password is not None:
        raise ValueError("origin credentials are not supported")
    port = origin.port or (443 if origin.scheme == "https" else 80)
    host = origin.hostname
    authority = f"[{host}]:{port}" if ":" in host else f"{host}:{port}"
    proxy_value = ""
    if probe["useProxy"]:
        key = "HTTPS_PROXY" if origin.scheme == "https" else "HTTP_PROXY"
        proxy_value = os.environ.get(key, os.environ.get(key.lower(), ""))
        if not proxy_value:
            raise ValueError("explicit proxy probe requires a configured proxy")
    proxy = urllib.parse.urlsplit(proxy_value) if proxy_value else None
    if proxy and (proxy.scheme != "http" or not proxy.hostname):
        raise ValueError("expected an HTTP proxy URL")

    target = (proxy.hostname, proxy.port or 80) if proxy else (host, port)
    connection = socket.create_connection(target, timeout=TIMEOUT)
    global CONNECTED
    CONNECTED = True
    try:
        proxy_auth = ""
        if proxy and proxy.username is not None:
            credentials = urllib.parse.unquote(proxy.username) + ":" + urllib.parse.unquote(proxy.password or "")
            proxy_auth = "Proxy-Authorization: Basic " + base64.b64encode(credentials.encode()).decode() + "\r\n"
        if proxy and origin.scheme == "https":
            connection.sendall((f"CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n" + proxy_auth + "\r\n").encode())
            response = http.client.HTTPResponse(connection)
            response.begin()
            if response.status != 200:
                return {"outcome": "proxyDenied", "status": response.status}
            # CONNECT has no body. Close the buffered reader, retaining the socket.
            response.close()
        if origin.scheme == "https":
            context = ssl.create_default_context()
            connection = context.wrap_socket(connection, server_hostname=probe.get("serverName") or host)
        path = urllib.parse.urlunsplit(("", "", origin.path or "/", origin.query, ""))
        request_target = probe["url"] if proxy and origin.scheme == "http" else path
        host_header = probe.get("hostHeader") or authority
        if "\r" in host_header or "\n" in host_header:
            raise ValueError("Host must be one header value")
        headers = proxy_auth if proxy and origin.scheme == "http" else ""
        connection.sendall((f"GET {request_target} HTTP/1.1\r\nHost: {host_header}\r\nConnection: close\r\n" + headers + "\r\n").encode())
        response = http.client.HTTPResponse(connection)
        response.begin()
        body = response.read(MAX_BODY + 1)
        if len(body) > MAX_BODY:
            return {"outcome": "protocolFailed"}
        return {"outcome": "response", "status": response.status, "bodySha256": hashlib.sha256(body).hexdigest(), "bytes": len(body)}
    finally:
        connection.close()


def network_failure(outcome):
    return {"outcome": "incompleteAfterConnect" if CONNECTED else outcome}


def observe(probe):
    try:
        if probe["kind"] == "tcp":
            with socket.create_connection((probe["host"], probe["port"]), timeout=TIMEOUT):
                return {"outcome": "connected"}
        if probe["kind"] == "dns":
            socket.getaddrinfo(probe["host"], 443, type=socket.SOCK_STREAM)
            return {"outcome": "connected"}
        if probe["kind"] == "dnsQuery":
            return dns_query(probe)
        if probe["kind"] == "http":
            return http_probe(probe)
        raise ValueError("unknown probe kind")
    except TimeoutError:
        return network_failure("timeout")
    except socket.gaierror:
        return {"outcome": "dnsFailed"}
    except ssl.SSLError:
        return {"outcome": "tlsFailed"}
    except (ConnectionRefusedError, ConnectionResetError, BrokenPipeError):
        return network_failure("refused")
    except http.client.HTTPException:
        return {"outcome": "protocolFailed"}
    except OSError as error:
        if error.errno in (errno.ENETUNREACH, errno.EHOSTUNREACH, errno.EACCES, errno.EPERM):
            return network_failure("unreachable")
        raise


if __name__ == "__main__":
    # Socket timeouts alone do not bound libc DNS or a peer trickling bytes.
    signal.signal(signal.SIGALRM, deadline)
    signal.alarm(TIMEOUT)
    result = observe(json.loads(sys.argv[1]))
    signal.alarm(0)
    print(json.dumps(result))
