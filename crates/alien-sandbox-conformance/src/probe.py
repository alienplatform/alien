"""One bounded network observation; invoked with a JSON request as argv[1]."""

import base64
import errno
import hashlib
import http.client
import json
import os
import signal
import socket
import ssl
import sys
import urllib.parse

TIMEOUT = 5
MAX_BODY = 1024 * 1024


def deadline(_signum, _frame):
    raise TimeoutError()


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


def observe(probe):
    try:
        if probe["kind"] == "tcp":
            with socket.create_connection((probe["host"], probe["port"]), timeout=TIMEOUT):
                return {"outcome": "connected"}
        if probe["kind"] == "dns":
            socket.getaddrinfo(probe["host"], 443, type=socket.SOCK_STREAM)
            return {"outcome": "connected"}
        if probe["kind"] == "http":
            return http_probe(probe)
        raise ValueError("unknown probe kind")
    except TimeoutError:
        return {"outcome": "timeout"}
    except socket.gaierror:
        return {"outcome": "dnsFailed"}
    except ssl.SSLError:
        return {"outcome": "tlsFailed"}
    except (ConnectionRefusedError, ConnectionResetError, BrokenPipeError):
        return {"outcome": "refused"}
    except http.client.HTTPException:
        return {"outcome": "protocolFailed"}
    except OSError as error:
        if error.errno in (errno.ENETUNREACH, errno.EHOSTUNREACH, errno.EACCES, errno.EPERM):
            return {"outcome": "unreachable"}
        raise


if __name__ == "__main__":
    # Socket timeouts alone do not bound libc DNS or a peer trickling bytes.
    signal.signal(signal.SIGALRM, deadline)
    signal.alarm(TIMEOUT)
    result = observe(json.loads(sys.argv[1]))
    signal.alarm(0)
    print(json.dumps(result))
