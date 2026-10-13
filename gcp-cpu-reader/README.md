GCP CPU quota reader/evaluator controls, implementation b7e4b0bef.

Real E2E target, us-east4: N2 usage 0 / applied limit 200. Demand 202 fails; demand 2 passes. The provider omits CPUS_ALL_REGIONS; both runs report non-blocking Unknown. No cloud writes or overrides.

Command: `cargo run -p alien-preflights --features runtime-checks --example q5_gcp_probe`. The temporary probe was removed after the run. It used a target OAuth token and explicitly trusted the sandbox CA in its HTTP client; the embedded Rust trust store cannot exchange service-account credentials in this sandbox. These are reader/evaluator controls, not actual deployment, parent comparison, or user-entry-point proof.
