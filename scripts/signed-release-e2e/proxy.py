#!/usr/bin/env python3
"""A compromised control plane between an Operator and its manager.

Forwards every request to the manager unchanged, except while an attack is
loaded: then it rewrites the manager's `/v1/sync` responses or other JSON
responses, or serves other bytes for an image manifest, a blob or a file. The
attack is the JSON file at $ATTACK_FILE, re-read on every request:

  {"name": "...",
   "sync": [{"op": "set" | "append" | "delete", "path": ["target", ...], "value": ...}],
   "http": [{"path": "<regex on the request path>", "ops": [<as in sync>]}],
   "manifests": {"<repository>": {"mediaType": "...", "body": "<manifest JSON>"}},
   "blobs": {"<digest>": "<base64>"},
   "files": {"<request path>": "<base64>"}}

`path` walks objects by key and lists by index. The last sync response the
manager sent is kept at $CAPTURE_DIR/sync.json, its last target at
$CAPTURE_DIR/target.json, and the last response sent to the Operator at
$CAPTURE_DIR/sync-sent.json.
"""

import base64
import http.client
import json
import os
import re
import sys
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

UPSTREAM = urllib.parse.urlsplit(os.environ.get("UPSTREAM", "http://alien-manager:8080"))
ATTACK_FILE = os.environ.get("ATTACK_FILE", "/attack/attack.json")
CAPTURE_DIR = os.environ.get("CAPTURE_DIR", "/attack/capture")
HOP_BY_HOP = {
    "connection", "keep-alive", "proxy-connection", "te", "trailer",
    "transfer-encoding", "upgrade", "content-length",
}
MANIFEST_PATH = re.compile(r"^/v2/(?P<repository>.+)/manifests/(?P<reference>[^/]+)$")
BLOB_PATH = re.compile(r"^/v2/.+/blobs/(?P<digest>[^/]+)$")


def load_attack():
    try:
        with open(ATTACK_FILE) as f:
            return json.load(f)
    except (FileNotFoundError, json.JSONDecodeError):
        return {}


def apply(document, operation):
    *parents, last = operation["path"]
    node = document
    for key in parents:
        node = node[key]
    if operation["op"] == "set":
        node[last] = operation["value"]
    elif operation["op"] == "append":
        node.setdefault(last, []).append(operation["value"])
    elif operation["op"] == "delete":
        if isinstance(node, list) or last in node:
            del node[last]
    else:
        raise ValueError(f"unknown op {operation['op']}")


def write_capture(name, body):
    os.makedirs(CAPTURE_DIR, exist_ok=True)
    path = os.path.join(CAPTURE_DIR, name)
    with open(path + ".tmp", "wb") as f:
        f.write(body)
    os.replace(path + ".tmp", path)


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):
        sys.stderr.write("proxy: " + (fmt % args) + "\n")

    def forward(self):
        attack = load_attack()
        path = urllib.parse.urlsplit(self.path).path
        manifest = MANIFEST_PATH.match(path)
        if manifest and self.command in ("GET", "HEAD"):
            swapped = attack.get("manifests", {}).get(manifest["repository"])
            if swapped:
                body = swapped["body"].encode()
                self.log_message("attack %s: served other manifest bytes for %s", attack.get("name"), path)
                self.reply(200, [("Content-Type", swapped["mediaType"])], body)
                return

        blob = BLOB_PATH.match(path)
        if blob and self.command in ("GET", "HEAD") and blob["digest"] in attack.get("blobs", {}):
            body = base64.b64decode(attack["blobs"][blob["digest"]])
            self.log_message("attack %s: served a blob the manager doesn't have, %s", attack.get("name"), path)
            self.reply(200, [("Content-Type", "application/octet-stream"),
                             ("Docker-Content-Digest", blob["digest"])], body)
            return
        if self.command == "GET" and path in attack.get("files", {}):
            self.log_message("attack %s: served %s", attack.get("name"), path)
            self.reply(200, [("Content-Type", "application/octet-stream")],
                       base64.b64decode(attack["files"][path]))
            return

        length = int(self.headers.get("Content-Length") or 0)
        request_body = self.rfile.read(length) if length else None
        headers = {k: v for k, v in self.headers.items() if k.lower() not in HOP_BY_HOP | {"host"}}
        headers["Host"] = UPSTREAM.netloc
        upstream = http.client.HTTPConnection(UPSTREAM.hostname, UPSTREAM.port or 80, timeout=300)
        try:
            upstream.request(self.command, self.path, body=request_body, headers=headers)
            response = upstream.getresponse()
            body = response.read()
        finally:
            upstream.close()
        response_headers = [(k, v) for k, v in response.getheaders() if k.lower() not in HOP_BY_HOP]
        if self.command == "HEAD":
            response_headers += [(k, v) for k, v in response.getheaders() if k.lower() == "content-length"]

        if path == "/v1/sync" and self.command == "POST" and response.status == 200:
            write_capture("sync.json", body)
            if b'"target"' in body and json.loads(body).get("target"):
                write_capture("target.json", json.dumps(json.loads(body)["target"]).encode())
            operations = attack.get("sync", [])
            if operations:
                document = json.loads(body)
                applied = 0
                for operation in operations:
                    try:
                        apply(document, operation)
                        applied += 1
                    except (KeyError, IndexError, TypeError):
                        pass
                if applied:
                    body = json.dumps(document).encode()
                    self.log_message("attack %s: rewrote /v1/sync (%d changes)", attack.get("name"), applied)
            write_capture("sync-sent.json", body)
        rewrites = [r for r in attack.get("http", []) if re.search(r["path"], path)]
        if rewrites and response.status == 200:
            document = json.loads(body)
            for rewrite in rewrites:
                for operation in rewrite["ops"]:
                    apply(document, operation)
            body = json.dumps(document).encode()
            self.log_message("attack %s: rewrote %s", attack.get("name"), path)
        self.reply(response.status, response_headers, body)

    def reply(self, status, headers, body):
        self.send_response(status)
        for key, value in headers:
            self.send_header(key, value)
        if self.command != "HEAD" or not any(k.lower() == "content-length" for k, _ in headers):
            self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    do_GET = do_POST = do_PUT = do_PATCH = do_DELETE = do_HEAD = forward


if __name__ == "__main__":
    ThreadingHTTPServer(("0.0.0.0", int(os.environ.get("PORT", "8080"))), Handler).serve_forever()
