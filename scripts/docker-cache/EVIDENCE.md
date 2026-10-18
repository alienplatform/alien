# Build cache evidence

All fixtures and credentials are synthetic. No cloud deployment was used.
The unchanged-main DockerToolchain baseline is
`5629803a7a704879a308dd1d724a88b81d9585ae`; the tested fixed implementation is
`9ed5b4035` (later evidence-only commits do not change it).
[results.json](results.json) records full layer digests, nonce values, fixture hashes,
versions, exit statuses, and HTTP payload counts. [README.md](README.md) provides the
complete local emulator, registry, push probe, baseline driver, and matrix commands.

## Observed results

Single runs on one Linux amd64 sandbox; these are correctness/traffic experiments,
not statistically robust performance benchmarks. The pinned fixture generates a
roughly 5.7 MB compressed image, dominated by its dependency layer and Alpine base.

| Scenario | Build seconds | Cache HTTP payload sent / received | Image push payload |
| --- | ---: | ---: | ---: |
| Baseline identical source, first cold builder | 4.93 | 0 / 0 | 5,730,011 B |
| Baseline identical source, second cold builder | 5.98 | 0 / 0 | 2,103,753 B |
| Fixed local first build | 5.39 | 0 / 0 | 5,730,013 B |
| Fixed local warm | 1.18 | 0 / 0 | 2,968 B |
| Fixed empty builder + stored cache | 3.38 | 5,740,681 / 5,741,433 B | 2,967 B |
| App-only edit, empty builder + stored cache | 4.77 | 5,740,681 / 5,741,430 B | 3,086 B |
| Dependency edit | 4.70 | 5,740,681 / 5,741,432 B | 2,103,867 B |
| Build-argument edit | 4.80 | 5,740,681 / 5,741,429 B | 2,103,753 B |
| Dockerfile dependency-step edit | 5.83 | 5,740,681 / 5,741,432 B | 2,103,755 B |

The two actual baseline driver runs used identical inputs but produced different
random dependency nonces and different dependency-layer digests. Baseline's second
push therefore uploaded about 2.1 MB of novel layer bytes. Fixed local and fresh-builder
warm runs preserved the dependency nonce and all layer bytes. App-only changes
preserved dependencies; dependency, argument, and Dockerfile changes produced new
nonces and correct independent outputs. Config and manifest uploads account for most
of the remaining ~3 KB warm push; zero new layers does not mean zero HTTP traffic.

The registry retained blobs between cases, so later cold image pushes can reuse the
base layer already uploaded earlier. Cache traffic is measured separately. Each cache
snapshot object is 5,740,544 bytes; multipart request/response XML adds small overhead.
Counts are actual HTTP payload bytes observed by a local proxy, excluding headers,
TLS, base-image pulls, and provider billing. Moto exercises the S3 protocol locally,
not real AWS authorization, durability, latency, or billing.

**The full-snapshot remote path costs more bandwidth in this fixture:** approximately
11.48 MB of cache transfer to avoid about 2.1 MB of changed layer upload. Local persistence
has no cache-transfer cost. Prefer an existing provider-selected persistent builder
when available; the snapshot interface trades transfer/storage for dependency reuse
on otherwise empty runners. It is explicitly opt-in through `--cache-url`.

## Behavioral controls

- The original fresh builder was removed before the warm remote build. Independent
  extraction verified the same random nonce, config, and layer payloads.
- Every measured archive's config/layer bytes matched its capture before OCI index
  normalization. Every pushed layer/config digest and payload was independently fetched
  and compared with the archive. No timestamp normalization was used.
- Archive application/dependency/version values were checked against independently
  specified source inputs and requested build arguments. Read-only, network-disabled
  containers produced those exact values and verified the dependency-file checksum.
  Docker 20.10 cannot pull these OCI manifests directly; its supported dual-archive load
  path was used after registry byte verification. This is not a claim of a successful
  OCI registry pull by that older daemon.
- A failing Dockerfile did not publish a replacement snapshot. A subsequent build reused
  the last good dependencies. Two fresh builders concurrently published distinct apps;
  both runtime outputs were correct and a third builder could import the result.
- Two actual processes concurrently created the same absent fallback builder, built
  distinct apps, ran them successfully, and left the shared builder available.
- An explicitly selected Docker context worked even with an invalid ambient Docker host.
- Five transport tests cover URL-prefix/project/target isolation, snapshot replacement,
  partial/unsafe archives, a known outside-staging canary, multipart part/commit failure,
  cleanup attempts, preservation of the primary error when abort fails, and visibility
  of the previous object after failed replacement. The upload buffer is one 8 MiB part.
- All 27 Docker-focused nextest tests pass on the repository-pinned nightly-2026-10-03,
  including real Docker builds and archive normalization tests.

## Limits and operational requirements

A fresh unauthenticated runner still needs one-time persistent-builder setup or a
credentialed durable cache URL. Provider setup instructions and actual observed soft
BuildKit GC targets are in the README. Active cache entries can exceed GC targets;
there is no hard disk quota. Old project scopes, bucket versions, and uploads left
incomplete by abrupt termination need storage lifecycle policies. Failed upload aborts
are reported while preserving the original error. Do not confuse a normal failure
cleanup test with crash/power-loss recovery guarantees.

This evidence covers native amd64 execution, not a native arm64 runtime or cloud
provider IAM. Scope isolation for alternate targets is tested independently. The
separate layer-cache correctness fix must be adopted by downstream consumers through
a new dependency release/revision; existing pinned versions are not silently updated.
