#!/usr/bin/env python3
"""Independently verify OCI payload digests and optional registry round-trip bytes."""
import argparse
import gzip
import hashlib
import io
import json
import tarfile
import urllib.request

parser = argparse.ArgumentParser()
parser.add_argument('archive')
parser.add_argument('--before')
parser.add_argument('--registry', help='http://host/v2/repository/manifests/tag')
args = parser.parse_args()

def digest(data):
    return 'sha256:' + hashlib.sha256(data).hexdigest()

def read_image(path):
    with tarfile.open(path) as archive:
        def blob(descriptor):
            value = archive.extractfile('blobs/sha256/' + descriptor['digest'].split(':')[1]).read()
            assert digest(value) == descriptor['digest']
            assert len(value) == descriptor['size']
            return value
        index = json.load(archive.extractfile('index.json'))
        descriptor = index['manifests'][0]
        value = blob(descriptor)
        manifest = json.loads(value)
        if 'manifests' in manifest:
            descriptor = next(d for d in manifest['manifests'] if d.get('platform', {}).get('architecture') == 'amd64')
            manifest = json.loads(blob(descriptor))
        payloads = {d['digest']: blob(d) for d in [manifest['config'], *manifest['layers']]}
        files = {}
        for layer in manifest['layers']:
            data = payloads[layer['digest']]
            if layer['mediaType'].endswith('+gzip'):
                data = gzip.decompress(data)
            with tarfile.open(fileobj=io.BytesIO(data)) as contents:
                for entry in contents:
                    name = entry.name.lstrip('./')
                    if entry.isfile() and name in ['app.txt', 'dependency.txt', 'dependencies/version', 'dependencies/build-nonce', 'dependencies/built-at']:
                        files[name] = contents.extractfile(entry).read().decode().strip()
        return manifest, payloads, files

manifest, payloads, files = read_image(args.archive)
if args.before:
    before, before_payloads, _ = read_image(args.before)
    assert payloads == before_payloads, 'normalization changed config/layer payloads'
if args.registry:
    request = urllib.request.Request(args.registry, headers={'Accept': 'application/vnd.oci.image.manifest.v1+json'})
    with urllib.request.urlopen(request) as response:
        registry_bytes = response.read()
        assert digest(registry_bytes) == response.headers['Docker-Content-Digest']
    remote = json.loads(registry_bytes)
    assert remote['layers'] == manifest['layers'], 'registry layer descriptors differ'
    assert remote['config'] == manifest['config'], 'registry config differs'
    base = args.registry.rsplit('/manifests/', 1)[0]
    for key, value in payloads.items():
        with urllib.request.urlopen(base + '/blobs/' + key) as response:
            actual = response.read()
        assert actual == value and digest(actual) == key, 'registry changed payload bytes'
print(json.dumps({'layers': manifest['layers'], 'config': manifest['config'], 'files': files}, indent=2))
