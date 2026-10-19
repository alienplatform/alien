"""Run the CNI probe's listener for real and reach it in each IP family a pod may use."""

import http.client
import pathlib
import socket
import subprocess
import sys
import time
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).parents[1]))
import cni_probe  # noqa: E402

BODY = b"alien-network-policy-control\n"


def free_port():
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


def ipv6_loopback():
    if not socket.has_ipv6:
        return False
    try:
        with socket.socket(socket.AF_INET6) as probe:
            probe.bind(("::1", 0))
        return True
    except OSError:
        return False


def get(host, port):
    connection = http.client.HTTPConnection(host, port, timeout=2)
    try:
        connection.request("GET", "/")
        response = connection.getresponse()
        return response.status, response.read()
    finally:
        connection.close()


class ListenerTest(unittest.TestCase):
    def test_listener_answers_on_every_available_family(self):
        port = free_port()
        server = subprocess.Popen(
            [sys.executable, "-c", cni_probe.listener_script(BODY, port)]
        )
        self.addCleanup(server.wait, 5)
        self.addCleanup(server.kill)
        hosts = ["127.0.0.1"] + (["::1"] if ipv6_loopback() else [])
        for host in hosts:
            with self.subTest(host=host):
                deadline = time.monotonic() + 10
                while True:
                    self.assertIsNone(server.poll(), "the listener exited")
                    try:
                        self.assertEqual((200, BODY), get(host, port))
                        break
                    except OSError:
                        self.assertLess(time.monotonic(), deadline, f"{host} never answered")
                        time.sleep(0.05)


if __name__ == "__main__":
    unittest.main()
