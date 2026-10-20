"""Build the real local queue fixture once and print its executable path."""
import json
import subprocess
import sys
from pathlib import Path

root = Path(__file__).resolve().parents[1]
result = subprocess.run(
    ["cargo", "build", "-p", "alien-local", "--example", "queue-http-test-server", "--message-format=json"],
    cwd=root, text=True, stdout=subprocess.PIPE, stderr=sys.stderr, check=True,
)
for line in result.stdout.splitlines():
    artifact = json.loads(line)
    if artifact.get("executable") and artifact.get("target", {}).get("name") == "queue-http-test-server":
        print(artifact["executable"])
        break
else:
    raise RuntimeError("Cargo did not report the queue test server executable")
