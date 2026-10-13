"""Exercise the generated gateway bootstrap against a local TLS backend.

Requires Docker, Python 3 and OpenSSL; does not contact cloud accounts."""
import copy
import hashlib
import http.client
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import socket
import ssl
import subprocess
import tempfile
import threading
import time
import uuid
import sys

IMAGE = sys.argv[1]
config = json.load(sys.stdin)
container = 'alien-envoy-wire-' + uuid.uuid4().hex[:8]
last_written = threading.Event()
backend_calls = []
payload = b'forwarding-parity\x00' * 131072


class Backend(BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'

    def log_message(self, *args):
        pass

    def do_GET(self):
        self.respond()

    def do_POST(self):
        self.respond()

    def respond(self):
        backend_calls.append(self.path)
        if self.path == '/private/stream':
            self.send_response(200)
            self.send_header('Transfer-Encoding', 'chunked')
            self.end_headers()
            for index, body in enumerate([b'first\n', b'second\n', b'last\n']):
                if index == 2:
                    last_written.set()
                self.wfile.write(('%x\r\n' % len(body)).encode() + body + b'\r\n')
                self.wfile.flush()
                time.sleep(0.3)
            self.wfile.write(b'0\r\n\r\n')
            self.wfile.flush()
            return
        if self.headers.get('Transfer-Encoding', '').lower() == 'chunked':
            chunks = []
            while True:
                length = int(self.rfile.readline().split(b';')[0], 16)
                if not length:
                    assert self.rfile.readline() == b'\r\n'
                    break
                chunks.append(self.rfile.read(length))
                assert self.rfile.read(2) == b'\r\n'
            body = b''.join(chunks)
        else:
            body = self.rfile.read(int(self.headers.get('Content-Length', '0')))
        if self.path == '/private/large':
            response = payload
        else:
            response = json.dumps({'host': self.headers.get('Host'), 'path': self.path,
                'apiHeaders': self.headers.get_all('x-apigw-api-id'),
                'proto': self.headers.get('x-forwarded-proto'),
                'bodyHash': hashlib.sha256(body).hexdigest()}).encode()
        self.send_response(200)
        self.send_header('Content-Length', str(len(response)))
        self.send_header('Set-Cookie', 'first=1; Secure; HttpOnly')
        self.send_header('Set-Cookie', 'second=2; Secure; HttpOnly')
        self.end_headers()
        self.wfile.write(response)


def port():
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        return listener.getsockname()[1]


with tempfile.TemporaryDirectory(prefix='alien-envoy-') as temporary:
    root = Path(temporary)
    root.chmod(0o755)  # The unprivileged Envoy process reads only the public CA file.
    subprocess.run(['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '1',
        '-keyout', str(root / 'key.pem'), '-out', str(root / 'ca.pem'), '-subj', '/CN=localhost',
        '-addext', 'subjectAltName=DNS:localhost'], check=True, capture_output=True)
    backend = ThreadingHTTPServer(('127.0.0.1', 0), Backend)
    tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    tls.load_cert_chain(root / 'ca.pem', root / 'key.pem')
    backend.socket = tls.wrap_socket(backend.socket, server_side=True)
    thread = threading.Thread(target=backend.serve_forever, daemon=True)
    thread.start()
    proxy_port, health_port = port(), port()
    for listener, selected in zip(config['static_resources']['listeners'], [proxy_port, health_port]):
        listener['address']['socket_address']['port_value'] = selected
        listener['address']['socket_address']['address'] = '127.0.0.1'
    cluster = config['static_resources']['clusters'][0]
    address = cluster['load_assignment']['endpoints'][0]['lb_endpoints'][0]['endpoint']['address']['socket_address']
    address.update(address='localhost', port_value=backend.server_port)
    upstream_tls = cluster['transport_socket']['typed_config']
    upstream_tls['sni'] = 'localhost'
    upstream_tls['common_tls_context']['validation_context']['trusted_ca']['filename'] = '/test/ca.pem'
    wrong = copy.deepcopy(cluster)
    wrong['name'] = 'wrong_identity'
    wrong['load_assignment']['cluster_name'] = 'wrong_identity'
    wrong['transport_socket']['typed_config']['sni'] = 'wrong.invalid'
    config['static_resources']['clusters'].append(wrong)
    hcm = config['static_resources']['listeners'][0]['filter_chains'][0]['filters'][0]['typed_config']
    wrong_route = copy.deepcopy(hcm['route_config']['virtual_hosts'][0])
    wrong_route.update(name='wrong_identity', domains=['bad.private-api.example.test'])
    wrong_route['routes'][0]['route']['cluster'] = 'wrong_identity'
    hcm['route_config']['virtual_hosts'].append(wrong_route)
    started = False
    def request(path, headers, body=None, method='GET', selected_port=None):
        connection = http.client.HTTPConnection('127.0.0.1', selected_port or proxy_port, timeout=10)
        connection.request(method, path, body=body, headers=headers)
        return connection, connection.getresponse()
    try:
        subprocess.run(['docker', 'run', '-d', '--name', container, '--network', 'host',
            '--label', 'test=worker-gateway', '-v', temporary + ':/test:ro', IMAGE,
            '--config-yaml', json.dumps(config), '--log-level', 'warning'], check=True, capture_output=True)
        started = True
        for _ in range(100):
            try:
                connection, response = request('/', {}, selected_port=health_port)
                status = response.status
                response.read()
                connection.close()
                if status == 200:
                    break
            except (OSError, http.client.HTTPException):
                pass
            time.sleep(0.1)
        else:
            raise RuntimeError('Local Envoy health listener did not become ready')
        for host in ['private-api.example.test', 'tenant.private-api.example.test', 'private-api.example.test:443']:
            connection, response = request('/a%2Fb?x=%2B&x=1', {
                'Host': host, 'x-apigw-api-id': 'attacker-selected', 'x-forwarded-proto': 'https',
                'x-forwarded-for': '192.0.2.123'}, body=payload, method='POST')
            assert response.status == 200, 'Valid upstream TLS/request failed'
            echoed = json.loads(response.read())
            assert echoed['host'] == host and echoed['path'] == '/private/a%2Fb?x=%2B&x=1'
            assert echoed['apiHeaders'] == ['exampleapi']
            assert echoed['proto'] == 'https', 'TLS termination scheme was lost'
            assert echoed['bodyHash'] == hashlib.sha256(payload).hexdigest()
            assert [value for key, value in response.getheaders() if key.lower() == 'set-cookie'] == ['first=1; Secure; HttpOnly', 'second=2; Secure; HttpOnly']
            connection.close()
        connection = http.client.HTTPConnection('127.0.0.1', proxy_port, timeout=10)
        connection.putrequest('GET', '/', skip_host=True)
        connection.putheader('Host', 'private-api.example.test')
        connection.putheader('x-apigw-api-id', 'attacker-one')
        connection.putheader('x-apigw-api-id', 'attacker-two')
        connection.endheaders()
        response = connection.getresponse()
        assert response.status == 200
        assert json.loads(response.read())['apiHeaders'] == ['exampleapi']
        connection.close()
        connection, response = request('/large', {'Host': 'private-api.example.test'})
        assert response.status == 200 and response.read() == payload
        connection.close()
        connection, response = request('/stream', {'Host': 'tenant.private-api.example.test'})
        assert response.status == 200
        assert response.readline() == b'first\n' and not last_written.is_set(), 'Proxy buffered the response stream'
        assert response.read().splitlines() == [b'second', b'last']
        connection.close()
        before = len(backend_calls)
        for host, expected in [('unowned.example.test', 404), ('bad.private-api.example.test', 503)]:
            connection, response = request('/', {'Host': host})
            assert response.status == expected
            response.read()
            connection.close()
        assert len(backend_calls) == before, 'Unknown host or invalid TLS identity reached backend'
        print('PASS: ordinary/nested/port Host, exact encoded path/query, forced API ID including duplicates, HTTPS scheme, binary request/response, duplicate cookies, unbuffered stream, unknown-host refusal, upstream TLS identity refusal')
    except Exception:
        if started:
            logs = subprocess.run(['docker', 'logs', container], capture_output=True, text=True)
            print(logs.stdout[-4000:] + logs.stderr[-4000:])
        raise
    finally:
        if started:
            subprocess.run(['docker', 'rm', '-f', container], check=True, capture_output=True)
        backend.shutdown()
        backend.server_close()
