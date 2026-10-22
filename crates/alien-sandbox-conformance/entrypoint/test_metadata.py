"""Real HTTP responses exercise refusal, redaction and unknown-response handling."""
import contextlib
import http.server
import json
import threading
import unittest

from metadata import probe, verify


@contextlib.contextmanager
def server(status, body):
    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            self.send_response(status)
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *_):
            pass

    listener = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=listener.serve_forever)
    thread.start()
    try:
        yield listener.server_port
    finally:
        listener.shutdown()
        listener.server_close()
        thread.join()


class MetadataTests(unittest.TestCase):
    def observe(self, status, body, listing=False):
        with server(status, body) as port:
            return probe("127.0.0.1", "/metadata", {}, port=port, listing=listing)

    def test_refused_credential_endpoints_pass(self):
        for status in (401, 403, 404):
            with self.subTest(status=status):
                result = self.observe(status, b"refused")
                self.assertEqual(result["status"], status)
                verify({"requests": [result]})

    def test_successful_credential_answers_fail_without_exposing_body(self):
        for body in (b'{"access_token":"secret-token-value"}', b"secret-token-value", b'{"AccessKeyId":"secret-token-value"}'):
            with self.subTest(body=body):
                result = self.observe(200, body)
                with self.assertRaises(AssertionError) as error:
                    verify({"requests": [result]})
                self.assertNotIn("secret-token-value", json.dumps(result))
                self.assertNotIn("secret-token-value", str(error.exception))

    def test_unknown_status_and_redirect_do_not_prove_exclusion(self):
        for status in (302, 429, 500):
            with self.subTest(status=status):
                with self.assertRaises(AssertionError):
                    verify({"requests": [self.observe(status, b"unknown")]})

    def test_mmds_listing_keeps_only_known_directory_names(self):
        result = self.observe(200, b"placement/\ntags/\n", listing=True)
        self.assertEqual(result["knownDirectories"], ["placement/", "tags/"])
        verify({"requests": [result]})
        unexpected = self.observe(200, b"placement/\nsecret-token-value\n", listing=True)
        self.assertNotIn("secret-token-value", json.dumps(unexpected))
        with self.assertRaises(AssertionError):
            verify({"requests": [unexpected]})

    def test_empty_observations_fail(self):
        with self.assertRaises(AssertionError):
            verify({"requests": []})


if __name__ == "__main__":
    unittest.main()
