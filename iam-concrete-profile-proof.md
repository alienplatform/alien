# IAM size coverage for concrete permission profiles

Feature commit: `4f621b3d4`. Predecessor: `3261d0d1d`.

Actual `alien build` runs with cloud credential variables removed, using literal synthetic permission grants. Concrete-only and mixed-small profiles pass with one artifact; mixed-overflow rejects the definite global overflow with exit 1 and zero artifacts. The feature additionally reports incomplete role-size coverage in all three cases; the predecessor does not. The seven focused checks and all 492 runtime-enabled preflight library tests pass; CLI build passes.

The concrete documents depend on controller rendering and deduplication. Their size is not inferred from profile configuration. This warning does not claim complete role-document aggregation or native setup apply proof.

Successful-build warnings currently require `RUST_LOG=warn` because the CLI defaults tracing to off. Logs and the successful screenshot explicitly use that setting. Failure output includes warnings through the normal error renderer. Default successful-build warning presentation needs a separate fix.

No cloud requests, created resources, or cleanup were needed.
