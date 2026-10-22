"""Probe metadata directly; never retain or print response bodies or tokens."""
import concurrent.futures
import http.client
import json

TIMEOUT = 2
LIMIT = 64 * 1024


def request(host, path, headers, method="GET", port=80):
    connection = http.client.HTTPConnection(host, port, timeout=TIMEOUT)
    try:
        connection.request(method, path, headers=headers)
        response = connection.getresponse()
        body = response.read(LIMIT + 1)
        return response.status, body
    finally:
        connection.close()


def probe(host, path, headers, *, method="GET", port=80, listing=False):
    observation = {"host": host, "path": path, "method": method}
    try:
        status, body = request(host, path, headers, method, port)
    except (OSError, http.client.HTTPException) as error:
        # Exception messages may contain response data. Keep only the error type/errno.
        observation.update(transportError=type(error).__name__, errno=getattr(error, "errno", None))
        observation["noCredentials"] = isinstance(error, OSError)
        return observation
    observation.update(status=status, responseBytes=len(body))
    if listing and status == 200:
        # Record the known MMDS directory names, never arbitrary response content.
        entries = body.decode("utf-8", errors="replace").split()
        observation["knownDirectories"] = [name for name in entries if name in ("placement/", "tags/")]
        observation["noCredentials"] = bool(entries) and all(name in ("placement/", "tags/") for name in entries)
    else:
        # A successful token/role endpoint, unknown status or redirect is a blocker.
        # This also fails closed if the endpoint returned an unfamiliar token format.
        observation["noCredentials"] = status in (401, 403, 404)
    return observation


def aws(host):
    headers = {}
    token_result = {"host": host, "path": "/latest/api/token", "method": "PUT"}
    try:
        status, token = request(host, "/latest/api/token", {"X-aws-ec2-metadata-token-ttl-seconds": "60"}, "PUT")
        token_result["status"] = status
        if status == 200 and len(token) <= LIMIT:
            headers["X-aws-ec2-metadata-token"] = token.decode("ascii")
    except (OSError, http.client.HTTPException, UnicodeError) as error:
        token_result["transportError"] = type(error).__name__
    # The session token alone is not an IAM credential. Probe both IMDS versions.
    results = []
    for auth in ([{}, headers] if headers else [{}]):
        for path in ("/latest/meta-data/", "/latest/meta-data/iam/security-credentials/", "/latest/meta-data/identity-credentials/ec2/security-credentials/ec2-instance"):
            result = probe(host, path, auth, listing=path == "/latest/meta-data/")
            result["imdsVersion"] = 2 if auth else 1
            results.append(result)
    return {"tokenRequest": token_result, "requests": results}


def collect(backend):
    if backend not in ("aws", "gcp", "azure"):
        raise ValueError("metadata backend must be aws, gcp or azure")
    hosts = ["169.254.169.254", "fd00:ec2::254", "fd20:ce::254"]
    if backend == "aws":
        with concurrent.futures.ThreadPoolExecutor(max_workers=len(hosts)) as pool:
            groups = list(pool.map(aws, hosts))
        return {"backend": backend, "requests": [item for group in groups for item in group["requests"]], "tokenRequests": [group["tokenRequest"] for group in groups]}
    if backend == "gcp":
        hosts.append("metadata.google.internal")
        headers = {"Metadata-Flavor": "Google"}
        paths = ["/computeMetadata/v1/instance/service-accounts/default/token", "/computeMetadata/v1/instance/service-accounts/default/identity?audience=alien-conformance&format=full"]
    else:
        headers = {"Metadata": "true"}
        paths = ["/metadata/identity/oauth2/token?api-version=2018-02-01&resource=https%3A%2F%2Fmanagement.azure.com%2F"]
    with concurrent.futures.ThreadPoolExecutor(max_workers=len(hosts) * len(paths)) as pool:
        futures = [pool.submit(probe, host, path, headers) for host in hosts for path in paths]
        results = [future.result() for future in futures]
    return {"backend": backend, "requests": results}


def verify(report):
    assert report["requests"], "metadata probe produced no observations"
    refused = [item for item in report["requests"] if not item["noCredentials"]]
    assert not refused, f"metadata credential exclusion unproven: {json.dumps(refused)}"
