from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import alienplatform


class Handler(BaseHTTPRequestHandler):
    def do_GET(self) -> None:
        body = f"{alienplatform.__name__}: ready\n".encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


ThreadingHTTPServer(("0.0.0.0", 8080), Handler).serve_forever()
