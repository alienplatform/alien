#!/usr/bin/env python3
"""Writes the tables a controller module's cloud calls are resolved against:

  crates/alien-infra/src/step_protocol/wasm/aws_endpoints.json
  crates/alien-infra/src/step_protocol/wasm/gcp_apis.json

AWS: the endpoint list botocore ships (botocore/data/endpoints.json at the tag below), partition
`aws` only, each service's default hostname per region. FIPS and dual-stack variants are separate
entries there and are left out, as are deprecated endpoints and any without https. A request is
signed for the service's signing name, which the endpoint list has for few services: the rest
come from the service models of the same tag (`metadata.signingName` of the model with the
endpoint prefix). A prefix with neither is left out, since nothing says what to sign it as.

The endpoint list is the one the AWS SDKs used before each service got its own endpoint rules.
A service AWS added since may be missing from it; the script prints those.

Google Cloud: the first label of each API host in Google's discovery directory. The directory
has no version, so the table records when it was read and the sha256 of what was read. Regional
hosts (`us-central1-aiplatform.googleapis.com`) are not in the directory, and so not in the
table. `www.googleapis.com`, the shared host of a few APIs outside Google Cloud, is left out.

Usage: scripts/generate-cloud-endpoint-tables.py [--botocore-tag TAG]

Run it to take a newer botocore tag or a newer directory, then run alien-infra's tests: they
check every host in the tables is one of the cloud's own.
"""
import argparse
import datetime
import gzip
import hashlib
import io
import json
import pathlib
import re
import tarfile
import urllib.request

BOTOCORE_TAG = "1.44.1"
BOTOCORE_URL = "https://codeload.github.com/boto/botocore/tar.gz/refs/tags/{tag}"
GOOGLE_DIRECTORY_URL = "https://www.googleapis.com/discovery/v1/apis"
OUT = pathlib.Path(__file__).resolve().parent.parent / "crates/alien-infra/src/step_protocol/wasm"


def fetch(url):
    request = urllib.request.Request(url, headers={"User-Agent": "endpoint-table-generator"})
    with urllib.request.urlopen(request, timeout=60) as response:
        return response.read()


def botocore_data(tarball):
    """The endpoint list, and each endpoint prefix's signing name from the service models."""
    endpoints = None
    signing_names = {}
    with tarfile.open(fileobj=io.BytesIO(tarball)) as archive:
        for member in archive:
            if member.name.endswith("/botocore/data/endpoints.json"):
                endpoints = archive.extractfile(member).read()
            model = re.search(r"/botocore/data/[^/]+/[^/]+/service-2\.json(\.gz)?$", member.name)
            if not model:
                continue
            raw = archive.extractfile(member).read()
            metadata = json.loads(gzip.decompress(raw) if model.group(1) else raw)["metadata"]
            prefix = metadata["endpointPrefix"]
            name = metadata.get("signingName", prefix)
            if signing_names.setdefault(prefix, name) != name:
                raise SystemExit(f"models disagree on the signing name of '{prefix}'")
    return endpoints, signing_names


def aws_table(raw, signing_names, tag):
    partition = next(p for p in json.loads(raw)["partitions"] if p["partition"] == "aws")
    regions = sorted(partition["regions"])
    suffix = partition["dnsSuffix"]
    services = {}
    for name, service in sorted(partition["services"].items()):
        defaults = service.get("defaults", {})
        template = defaults.get("hostname", partition["defaults"]["hostname"])
        protocols = defaults.get("protocols", partition["defaults"]["protocols"])
        entry = {"global": None, "regional": {}}
        for key, endpoint in service["endpoints"].items():
            if endpoint.get("deprecated") or "https" not in endpoint.get("protocols", protocols):
                continue
            # A key that is not a region names the service's one endpoint for the partition.
            if key not in regions and key != service.get("partitionEndpoint"):
                continue
            scope = endpoint.get("credentialScope", {})
            host = endpoint.get("hostname") or template.replace("{service}", name).replace(
                "{region}", key
            ).replace("{dnsSuffix}", suffix)
            signing_region = scope.get("region", key)
            signing_name = scope.get(
                "service", defaults.get("credentialScope", {}).get("service", signing_names.get(name))
            )
            if signing_region not in regions or signing_name is None:
                continue
            target = [host, signing_region, signing_name]
            if key in regions:
                entry["regional"][key] = target
            else:
                entry["global"] = target
        if entry["regional"] or entry["global"]:
            services[name] = entry
    source = (
        f"botocore {tag}: botocore/data/endpoints.json, sha256 {hashlib.sha256(raw).hexdigest()}, "
        f"and the signing names of its {len(signing_names)} endpoint prefixes with a service model"
    )
    missing = sorted(set(signing_names) - set(services))
    return source, regions, services, missing


def gcp_table(raw):
    services = set()
    for api in json.loads(raw)["items"]:
        host = re.match(r"https://([^/]+)/", api["discoveryRestUrl"]).group(1)
        label = re.fullmatch(r"([a-z0-9-]+)\.googleapis\.com", host)
        if label and label.group(1) != "www":
            services.add(label.group(1))
    today = datetime.date.today().isoformat()
    source = f"{GOOGLE_DIRECTORY_URL}, read {today}, sha256 {hashlib.sha256(raw).hexdigest()}"
    return source, sorted(services)


def compact(value):
    return json.dumps(value, separators=(",", ":"), sort_keys=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--botocore-tag", default=BOTOCORE_TAG)
    args = parser.parse_args()

    endpoints, signing_names = botocore_data(fetch(BOTOCORE_URL.format(tag=args.botocore_tag)))
    source, regions, services, missing = aws_table(endpoints, signing_names, args.botocore_tag)
    # One service per line, so a new tag's change reads as a diff.
    lines = ",\n".join(f"{compact(name)}:{compact(entry)}" for name, entry in services.items())
    (OUT / "aws_endpoints.json").write_text(
        f'{{"source":{compact(source)},\n"regions":{compact(regions)},\n"services":{{\n{lines}\n}}}}\n'
    )
    hosts = sum(len(e["regional"]) + bool(e["global"]) for e in services.values())
    print(f"aws: {len(services)} services, {len(regions)} regions, {hosts} hosts")
    print(f"aws: {len(missing)} endpoint prefixes with a model and no endpoint in the list:")
    print("  " + " ".join(missing))

    source, apis = gcp_table(fetch(GOOGLE_DIRECTORY_URL))
    lines = ",\n".join(compact(api) for api in apis)
    (OUT / "gcp_apis.json").write_text(
        f'{{"source":{compact(source)},\n"services":[\n{lines}\n]}}\n'
    )
    print(f"gcp: {len(apis)} APIs")


if __name__ == "__main__":
    main()
