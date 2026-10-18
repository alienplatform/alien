# Docker dependency cache validation

Docker builds retain their daemon-scoped fallback BuildKit builder (`alien-cache-v1-*`).
BuildKit's default garbage collection policies bound its local cache; Alien does not prune
or reconfigure a builder you selected. Use `docker buildx du --builder NAME` to inspect
usage and `docker buildx prune --builder NAME` when you deliberately want to discard it.
A failed build leaves the builder available for later builds.

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
