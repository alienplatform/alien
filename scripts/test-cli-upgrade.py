#!/usr/bin/env python3
"""Run the production updater against local HTTP releases and disposable CLI copies.

Pass a stable CLI, then two canary CLIs built with ALIEN_CLI_VERSION. Use a
canary base newer than stable and revisions ffffffff then 00000001 to exercise
rollback and revisions that cannot be ordered or parsed as SemVer integers.
Set --sdk-version to the workspace package version used to build the canaries.
"""

import argparse
import hashlib
import http.server
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import tempfile
import threading


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def run(binary, *args, env=None, success=True):
    result = subprocess.run(
        [str(binary), *args], capture_output=True, text=True, env=env, timeout=120
    )
    output = result.stdout + result.stderr
    assert (result.returncode == 0) == success, output
    return output


def version(binary):
    return run(binary, "--version").strip().split()[-1].removeprefix("v")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("stable", type=Path)
    parser.add_argument("canary", type=Path)
    parser.add_argument("next_canary", type=Path)
    parser.add_argument(
        "--sdk-version", required=True,
        help="Workspace package version used to build both canary fixtures",
    )
    args = parser.parse_args()
    sources = [path.resolve() for path in (args.stable, args.canary, args.next_canary)]
    stable, canary, next_canary = [version(path) for path in sources]
    assert re.fullmatch(r"\d+\.\d+\.\d+", stable), stable
    assert canary.endswith("-ffffffff"), canary
    assert next_canary == canary.removesuffix("ffffffff") + "00000001", next_canary
    assert tuple(map(int, canary.split("-")[0].split("."))) > tuple(
        map(int, stable.split("."))
    ), "Build canaries with a base newer than the stable fixture"
    os_name = {"Darwin": "darwin", "Linux": "linux", "Windows": "windows"}[
        platform.system()
    ]
    arch = {"arm64": "aarch64", "aarch64": "aarch64", "AMD64": "x86_64", "x86_64": "x86_64"}[
        platform.machine()
    ]
    executable = "alien.exe" if os_name == "windows" else "alien"
    artifacts = {
        f"/alien/v{value}/{os_name}-{arch}/{executable}": (path, sha256(path))
        for value, path in zip((stable, canary, next_canary), sources)
    }
    channels = {"stable": f"v{stable}", "canary": f"v{canary}"}
    requests = []
    fault = None

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            requests.append(self.path)
            if self.path.startswith("/channels/"):
                body = channels[self.path.removeprefix("/channels/")].encode()
                self.send_response(200)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
                return
            if self.path not in artifacts:
                self.send_error(404)
                return
            path, checksum = artifacts[self.path]
            if fault == "wrong-version":
                path = sources[0]
                checksum = sha256(path)
            self.send_response(200)
            self.send_header("Content-Length", str(path.stat().st_size))
            if fault != "missing-checksum":
                self.send_header(
                    "x-amz-meta-sha256", "0" * 64 if fault == "checksum" else checksum
                )
            self.end_headers()
            try:
                with path.open("rb") as stream:
                    shutil.copyfileobj(stream, self.wfile)
            except (BrokenPipeError, ConnectionResetError):
                if fault != "missing-checksum":
                    raise

        def log_message(self, *_args):
            pass

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    env = {
        **os.environ,
        "ALIEN_RELEASES_URL": f"http://127.0.0.1:{server.server_port}",
        "ALIEN_INSTALL_METHOD": "standalone",
        "NO_PROXY": "127.0.0.1",
    }
    try:
        with tempfile.TemporaryDirectory(prefix="alien-upgrade-test-") as directory:
            installed = Path(directory) / executable
            shutil.copy2(sources[0], installed)
            assert "--canary" in run(installed, "update", "--help")
            assert stable in run(installed, "update", "--version")
            initial = sha256(installed)
            output = run(installed, "update", "--canary", "--dry-run", env=env)
            assert f"to v{canary}" in output, output
            assert requests == ["/channels/canary"], requests
            assert sha256(installed) == initial
            print("PASS dry run selects canary without downloading or replacing")

            requests.clear()
            run(installed, "update", "--canary", env=env)
            assert version(installed) == canary
            assert sha256(installed) == sha256(sources[1])
            assert len(requests) == 2, requests
            print("PASS stable installs the canary and verifies its executable")

            requests.clear()
            output = run(installed, "update", "--canary", env=env)
            assert "already up to date" in output, output
            assert requests == ["/channels/canary"], requests
            requests.clear()
            run(installed, "update", "--canary", "--force", env=env)
            assert len(requests) == 2, requests
            assert sha256(installed) == sha256(sources[1])
            print("PASS equal canary is a no-op unless forced")

            channels["canary"] = f"v{next_canary}"
            run(installed, "upgrade", "--canary", env=env)
            assert version(installed) == next_canary
            assert sha256(installed) == sha256(sources[2])
            print("PASS reverse revision order and leading-zero numeric identity")

            plugin = Path(directory) / "version-check"
            run(installed, "operations", "init", "version-check", str(plugin), "--json", env=env)
            assert f'alien-operations-sdk = "={args.sdk_version}"' in (plugin / "Cargo.toml").read_text()
            print("PASS canary CLI scaffolds the expected SDK dependency version")

            run(installed, "update", env=env)
            assert version(installed) == stable
            assert sha256(installed) == initial
            print("PASS plain update returns to stable even with a lower base version")

            for value in (f"v{stable}", "v0.0.0"):
                channels["stable"] = value
                requests.clear()
                output = run(installed, "update", env=env)
                assert requests == ["/channels/stable"], requests
                assert sha256(installed) == initial
            requests.clear()
            run(installed, "update", "--force", env=env)
            assert requests == ["/channels/stable"], requests
            channels["stable"] = f"v{stable}"
            requests.clear()
            run(installed, "update", "--force", env=env)
            assert len(requests) == 2, requests
            assert sha256(installed) == initial
            print("PASS stable update preserves no-op, force, and downgrade rules")

            for fault, expected in (
                ("checksum", "checksum mismatch"),
                ("missing-checksum", "valid SHA-256 checksum"),
                ("wrong-version", "failed validation"),
            ):
                output = run(installed, "update", "--canary", env=env, success=False)
                assert expected in output, output
                assert sha256(installed) == initial
            fault = None
            print("PASS invalid checksum, missing checksum, and wrong version preserve the executable")

            for invalid in ("latest", f"v{stable}", f"v{stable}-ABCDEF12"):
                channels["canary"] = invalid
                requests.clear()
                output = run(installed, "update", "--canary", env=env, success=False)
                assert "invalid version" in output, output
                assert requests == ["/channels/canary"], requests
                assert sha256(installed) == initial
            print("PASS malformed and wrong-channel pointers fail before downloading")

            homebrew = Path(directory) / "Cellar" / "alien" / stable / "bin" / executable
            homebrew.parent.mkdir(parents=True)
            shutil.copy2(sources[0], homebrew)
            for binary, method, command in (
                (installed, "npm", "npm install -g @alienplatform/cli@latest"),
                (homebrew, "standalone", "brew upgrade alienplatform/tap/alien"),
            ):
                package_env = {**env, "ALIEN_INSTALL_METHOD": method}
                requests.clear()
                output = run(
                    binary, "update", "--canary", "--force", "--dry-run",
                    env=package_env, success=False,
                )
                assert "standalone installation" in output, output
                assert requests == [], requests
                assert sha256(binary) == initial
                assert command in run(binary, "update", "--dry-run", env=package_env)
            print("PASS npm and Homebrew refuse canary and preserve stable delegation")
    finally:
        server.shutdown()
        server.server_close()
        thread.join()


if __name__ == "__main__":
    main()
