#!/usr/bin/env python3
"""A manager that only answers the Operator: `/v1/initialize`, and `/v1/sync`
with the response body in $SYNC_DIR/<name>.json, where <name> is the content
of $SYNC_DIR/current. It plays a fully compromised control plane: the test
picks what it sends. Every sync request is appended to $SYNC_DIR/requests.log.
"""

import json
import os
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

SYNC_DIR = os.environ.get("SYNC_DIR", "/sync")
DEPLOYMENT_ID = os.environ.get("DEPLOYMENT_ID", "dep_kind")


def current():
    try:
        with open(os.path.join(SYNC_DIR, "current")) as f:
            name = f.read().strip()
        with open(os.path.join(SYNC_DIR, f"{name}.json")) as f:
            return json.load(f)
    except (FileNotFoundError, json.JSONDecodeError):
        return {}


class Handler(BaseHTTPRequestHandler):
    def reply(self, body):
        data = json.dumps(body).encode()
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_POST(self):
        length = int(self.headers.get("content-length") or 0)
        request = self.rfile.read(length)
        if self.path == "/v1/initialize":
            return self.reply({"deploymentId": DEPLOYMENT_ID, "deploymentModel": "pull", "token": "ax_dep_kind"})
        if self.path == "/v1/sync":
            with open(os.path.join(SYNC_DIR, "requests.log"), "ab") as log:
                log.write(request + b"\n")
            return self.reply(current())
        return self.reply({})

    def do_GET(self):
        return self.reply({})

    def log_message(self, *args):
        pass


if __name__ == "__main__":
    ThreadingHTTPServer(("0.0.0.0", 8080), Handler).serve_forever()
