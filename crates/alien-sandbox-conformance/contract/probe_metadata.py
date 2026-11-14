"""Probe cloud metadata endpoints as the current identity and print a bounded report."""
import json
import os
import sys

from metadata import collect, verify


# Variables that hand a guest process a cloud credential endpoint or token.
CREDENTIAL_ENV = ("IDENTITY_ENDPOINT", "IDENTITY_HEADER", "MSI_ENDPOINT", "MSI_SECRET",
                  "AWS_CONTAINER_CREDENTIALS_RELATIVE_URI", "AWS_CONTAINER_CREDENTIALS_FULL_URI",
                  "AWS_ACCESS_KEY_ID", "AWS_SESSION_TOKEN", "GOOGLE_APPLICATION_CREDENTIALS")


def main():
    backend = sys.argv[1] if len(sys.argv) > 1 else os.environ.get("ALIEN_CONFORMANCE_METADATA", "")
    # Names only: never print a value.
    present = [name for name in CREDENTIAL_ENV if name in os.environ]
    report = {"uid": os.getuid(), "credentialEnvironment": present, "metadata": collect(backend)}
    print(json.dumps(report))
    assert not present, f"credential environment reachable: {present}"
    verify(report["metadata"])


if __name__ == "__main__":
    main()
