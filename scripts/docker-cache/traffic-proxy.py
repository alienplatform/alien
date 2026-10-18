#!/usr/bin/env python3
"""Count HTTP payload bytes for a local synthetic cache/registry experiment.

Headers (including credentials) are never logged. Use only against local fixtures.
"""
import argparse
import http.client
import http.server
import json
import threading
import time

parser = argparse.ArgumentParser()
parser.add_argument('--listen', type=int, required=True)
parser.add_argument('--upstream', type=int, required=True)
parser.add_argument('--log', required=True)
args = parser.parse_args()
lock = threading.Lock()

class Proxy(http.server.BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'

    def log_message(self, *_):
        pass

    def forward(self):
        chunks = []
        if self.headers.get('Transfer-Encoding', '').lower() == 'chunked':
            while True:
                size = int(self.rfile.readline().split(b';', 1)[0], 16)
                if not size:
                    while self.rfile.readline() != b'\r\n':
                        pass
                    break
                chunks.append(self.rfile.read(size))
                assert self.rfile.read(2) == b'\r\n'
            body = b''.join(chunks)
        else:
            body = self.rfile.read(int(self.headers.get('Content-Length', 0)))
        headers = {k: v for k, v in self.headers.items()
                   if k.lower() not in ('transfer-encoding', 'connection', 'expect', 'content-length')}
        headers['Content-Length'] = str(len(body))
        connection = http.client.HTTPConnection('127.0.0.1', args.upstream, timeout=120)
        connection.request(self.command, self.path, body=body, headers=headers)
        response = connection.getresponse()
        payload = response.read()
        with lock, open(args.log, 'a') as output:
            output.write(json.dumps({'time': time.time(), 'method': self.command,
                'path': self.path.split('?', 1)[0], 'status': response.status,
                'requestBytes': len(body), 'responseBytes': len(payload)}) + '\n')

        self.send_response(response.status)
        for key, value in response.getheaders():
            if key.lower() not in ('transfer-encoding', 'connection', 'content-length'):
                self.send_header(key, value)
        self.send_header('Content-Length', response.getheader('Content-Length', '0') if self.command == 'HEAD' else str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)
        connection.close()

    do_GET = do_HEAD = do_POST = do_PUT = do_PATCH = do_DELETE = forward

http.server.ThreadingHTTPServer(('127.0.0.1', args.listen), Proxy).serve_forever()
