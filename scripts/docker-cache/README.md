# Docker dependency cache validation

Docker builds retain their daemon-scoped fallback BuildKit builder (`alien-cache-v1-*`).
BuildKit's default garbage collection policies manage its local cache; Alien does not prune
or reconfigure a builder you selected. Use `docker buildx du --builder NAME` to inspect
usage and `docker buildx prune --builder NAME` when you deliberately want to discard it.
A failed build leaves the builder available for later builds. The builder remains running.
GC thresholds are soft targets, not a strict disk quota: active records cannot always be
reclaimed. On the tested BuildKit 0.33.1 worker, `buildx inspect` reported 488.3 MiB/48h
for short-lived source/cache-mount records and 73.57 GiB maximum-used / 18.63 GiB
minimum-free targets for other records on a roughly 100 GB disk. These are defaults,
not hardcoded Alien limits. Select a builder with your own BuildKit GC configuration
when those defaults do not fit your machine.

A fresh runner has no previous local cache. Select your CI provider's persistent builder
before `alien build`, or supply the existing `--cache-url` with durable storage and its
credentials. For example, `alien build --cache-url s3://build-cache/project-prefix`.
The URL's prefix is respected. Different stack/resource/target scopes have separate
snapshots. The cache is separate from release image repositories. BuildKit still checks
Dockerfile, source, dependency and build-argument inputs; no timestamps are rewritten.

For Blacksmith direct CLI builds, use its `useblacksmith/setup-docker-builder@v2` setup
step before invoking Alien. Merely having a daemon image cache does not persist BuildKit
layers. See https://docs.blacksmith.sh/blacksmith-caching/docker-builds.
Other CI providers need equivalent selected-builder persistence or the explicit store.
There is no automatic remote cache for an unauthenticated fresh runner. GitHub Actions
shell steps do not automatically receive cache-service runtime credentials.

Object storage uses one current BuildKit `mode=max` snapshot per scope. Import/export use
separate temporary directories, and concurrent writers atomically replace the snapshot.
The last successful writer wins; concurrency can reduce hit rate but cannot combine
incomplete exports. Old local-export blobs are not retained across snapshots. Configure
bucket lifecycle rules for obsolete project scopes and object versions if versioning is
on. Snapshots are streamed with bounded buffers, but each build downloads and uploads
the whole snapshot, including unchanged layers. A 1 GiB cache can therefore transfer
about 2 GiB per warm build. Prefer a provider's incremental persistent builder when
available. Cache access errors fail explicitly; an absent object is a normal cold build.
Trust the cache store as build input and restrict its write access accordingly.

## Local experiment

Use a disposable Docker environment. The fixture deliberately writes a random nonce in
its dependency step: identical nonce plus identical layer bytes on later builds shows
that step was reused, without pretending arbitrary Dockerfiles are reproducible.

```sh
cargo build -p alien-build --example docker-cache-probe
DOCKER_HOST=unix:///var/run/docker.sock \
  target/debug/examples/docker-cache-probe scripts/docker-cache /tmp/cache-probe-one
DOCKER_HOST=unix:///var/run/docker.sock \
  target/debug/examples/docker-cache-probe scripts/docker-cache /tmp/cache-probe-two
cargo test -p alien-build --lib toolchain::docker_cache::tests
```

`PROBE_CACHE_URL` passes a cache URL through the same parser as `alien build`.
`PROBE_CACHE_SCOPE` changes the project/resource scope; `PROBE_DEP_VERSION` changes a
Docker build argument. Select distinct empty builders with `BUILDX_BUILDER` to prove
remote reuse independently of local cache. Delete only test-owned builders.

To capture the archive before normalization, put a symlink named `docker` to
`docker-wrapper` in a temporary PATH directory and set `REAL_DOCKER` to the original
executable. Compare all config/layer blob bytes before and after; the index can change.
Use `traffic-proxy.py` with a local S3 emulator or registry to count request/response
payload bytes. It logs no headers, but it is a test-only buffering proxy, not a service.
Read-only, network-disabled container runs must verify application/dependency output.
No cloud deployment is needed for these checks.

## Complete local transport setup

From this repository root, in a disposable environment (ports below must be free):

```sh
python3 -m venv /tmp/cache-probe-tools
/tmp/cache-probe-tools/bin/pip install 'moto[server]==5.1.17'
/tmp/cache-probe-tools/bin/moto_server -H 127.0.0.1 -p 51615 >/tmp/cache-moto.log 2>&1 &
docker run -d --name cache-probe-registry -p 127.0.0.1:51614:5000 \
  registry@sha256:a3d8aaa63ed8681a604f1dea0aa03f100d5895b6a58ace528858a7b332415373
python3 scripts/docker-cache/traffic-proxy.py --listen 51616 --upstream 51614 --log /tmp/cache-registry.jsonl &
python3 scripts/docker-cache/traffic-proxy.py --listen 51617 --upstream 51615 --log /tmp/cache-s3.jsonl &
/tmp/cache-probe-tools/bin/python - <<'PY'
import boto3
client = boto3.client('s3', endpoint_url='http://127.0.0.1:51617',
    aws_access_key_id='synthetic', aws_secret_access_key='synthetic', region_name='us-east-1')
client.create_bucket(Bucket='synthetic-build-cache')
PY

# The public push probe is committed source, not a hidden test dependency.
git clone https://github.com/alienplatform/dockdash.git /tmp/cache-push-probe
git -C /tmp/cache-push-probe checkout 04a56d0c750884d8ed66de13f86e0279132d135b
cargo +stable build --manifest-path /tmp/cache-push-probe/Cargo.toml --example cache-evidence-push
cargo build -p alien-build --example docker-cache-probe
AWS_ACCESS_KEY_ID=synthetic AWS_SECRET_ACCESS_KEY=synthetic AWS_REGION=us-east-1 \
AWS_ENDPOINT=http://127.0.0.1:51617 AWS_ALLOW_HTTP=true \
python3 scripts/docker-cache/run-matrix.py \
  --output /tmp/cache-matrix-results --prefix cache-proof \
  --cache-url s3://synthetic-build-cache/proof \
  --cache-log /tmp/cache-s3.jsonl --registry-log /tmp/cache-registry.jsonl \
  --push-probe /tmp/cache-push-probe/target/debug/examples/cache-evidence-push
```

The runner compares archive app/dependency bytes and version to the source files and
requested build argument, then compares read-only/network-none runtime output to those
independent inputs. It records exit statuses and checks preserved nonces/layers for cache
reuse, changed nonces for invalidation, normalization and registry payload identity,
failure recovery, and concurrent distinct app builds. It creates only prefixed test
builders; the first *local* result is cold only if the daemon's fallback was empty.
It never clears an existing fallback cache. Its HTTP counts cover cache and image-push
payloads, not headers, TLS, base-image pulls, or provider billing.

For simultaneous first-creation validation, use a disposable daemon or remove only a
fallback known to belong to your test, then start two probe processes with
`BUILDX_BUILDER=default`, distinct app source directories, and separate output directories.
Inspect and run both images against their independent inputs, and inspect the retained
shared builder. Delete only test-created containers/builders when finished.

## Reproduce the unchanged-main baseline

Use the same committed fixture with the old toolchain, not a hand-written approximation:

```sh
git worktree add --detach /tmp/cache-before 5629803a7a704879a308dd1d724a88b81d9585ae
python3 - <<'PY'
from pathlib import Path
probe = Path('crates/alien-build/examples/docker-cache-probe.rs').read_text()
probe = probe.replace('cache_utils::store_from_url, ', '')
probe = probe.replace('cache_store: store_from_url(std::env::var("PROBE_CACHE_URL").ok().as_deref())?,', 'cache_store: None,')
Path('/tmp/cache-before/crates/alien-build/examples/docker-cache-probe.rs').write_text(probe)
PY
cargo build --manifest-path /tmp/cache-before/Cargo.toml -p alien-build --example docker-cache-probe
BUILDX_BUILDER=default /tmp/cache-before/target/debug/examples/docker-cache-probe scripts/docker-cache /tmp/cold-one
BUILDX_BUILDER=default /tmp/cache-before/target/debug/examples/docker-cache-probe scripts/docker-cache /tmp/cold-two
python3 scripts/docker-cache/inspect-image.py /tmp/cold-one/linux-x64.oci.tar
python3 scripts/docker-cache/inspect-image.py /tmp/cold-two/linux-x64.oci.tar
```

Only the harness is added to the detached baseline checkout; the baseline Docker
implementation stays unchanged. Both builds execute the dependency step, changing its
random nonce and layer digest. Use the same wrapper/push/inspection commands above to
capture normalization and registry bytes. The baseline and fixed implementations need
not use the same Cargo target directory.
