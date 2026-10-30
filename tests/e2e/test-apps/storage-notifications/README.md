# Storage notification qualification

This fixture has two Frozen source buckets, three filtered notification rules
sharing one Frozen queue, and a separately built TypeScript Worker with a
10-message / 5-second queue batch trigger. The Worker uses
`storageEventFromMessage` and saves every invocation in a separate object in
`results`; duplicate deliveries cannot disappear behind an overwritten result.

`POST /objects` writes an array of `{ storage: "objects" | "second", key, content }`
through the actual SDK. `GET /batches` reads the durable invocation records.
These endpoints are for isolated test deployments only.

The regular Worker integration CI job runs:

```sh
cargo nextest run -p alien-local --test worker_integration \
  -E 'test(compiled_worker_consumes_notifications_after_cold_restart)'
```

It builds the app, starts its process and real queue collector, checks exact
matching keys and multi-message batches, closes the provider and Worker, writes
objects while stopped, restores both from disk, and verifies only the offline
changes arrive. A complete safety-scan interval must pass without replay and
successful receipts must leave the queue empty. The test deletes all resources.
It requires the same Bun, built SDK and native bindings addon as the other tests
in that integration binary.

The cloud E2E workflow's `storage-notifications` application runs the same
fixture through AWS/GCP/Azure controller provisioning or AWS CloudFormation and
AWS/GCP/Azure Terraform setup. Its seven test entry points are:

```sh
cargo nextest run -p alien-test --test push --test distribution \
  -E 'test(storage_notifications)'
```

Select one test when qualifying a single provider. The shared assertions require
all expected keys, both source bucket identities, canonical payload metadata,
quoted Unicode/percent/star keys, filter exclusions, and at least one actual
multi-message batch. They continue observing after delivery to catch delayed
excluded messages. Provider duplicates are permitted and remain in the records.
The existing E2E context runs deployment cleanup even if an assertion fails.

Cloud execution needs the standard E2E credentials and build prerequisites.
These entry points do not yet assert rule-update setup rejection/re-import,
independent cloud-resource absence, or multipart upload; retain separate cloud
qualification for those behaviors. A cloud test being compiled is not evidence
that its deployment or cleanup passed.
