# ALIEN-222 (10) — Wire command receiver env in controllers — Impl Report

SPEED MODE, single implementer. Worktree: `.claude/worktrees/alien-222-wiring` on
branch `alien-222-wiring` (base `origin/alien-211-runtime-less` @ 8b0bb472).
Three unsquashed local commits (controller squashes + pushes):

- `721080e3` feat(core): add Stack::receiver_command_env_vars for Container/Daemon receiver env
- `4efb19be` feat(manager,operator): inject Container/Daemon command receiver env
- `39bf4a11` feat(infra,local): cut passthrough, wire receiver token secret ref + local url rewrite

## Platform-gate finding (Constraint 4) + evidence

DECISION: the manager injects Container/Daemon receiver env on **every platform**
(ungated), NOT behind the worker `needs_polling` (K8s|Local) gate. The operator
injects it inside its existing K8s/Local block (the operator only ever manages
K8s/Local, so no gate widening is needed there).

Evidence traced in code:
1. **Delivery rule (ALIEN-219):** Container/Daemon always deliver via Pull
   (`CommandDeliveryMode::Pull`) regardless of platform, unlike Workers which use
   platform-native push on cloud platforms. The manager's `needs_polling` gate is
   self-justified in its own comment as *worker*-push-specific ("Cloud workers …
   receive commands via platform-native push"). That reasoning does not hold for
   Container/Daemon receivers.
2. **Snapshot reaches Container/Daemon env platform-independently:**
   `alien-deployment::inject_into_environment` (helpers.rs ~437) dispatches by
   downcasting each resource to `Worker`/`Container`/`Daemon` and injecting the
   snapshot vars per-resource — there is no platform branch. Plain vars go
   straight into the resource's `environment` HashMap; Secret vars are collected
   into `ALIEN_SECRETS`. So any platform that has a Container/Daemon resource
   receives the receiver env from the same manager snapshot path.
3. **OSS controller reality:** OSS `alien-infra` has Container/Daemon controllers
   only for K8s/Local. BYOC (aws/gcp/azure) Container/Daemon controllers live in
   Horizon (private, not in this worktree). Ungating the manager is forward-correct
   and harmless: `Stack::receiver_command_env_vars` returns empty when the stack
   declares no command-enabled Container/Daemon targets, so non-K8s/Local
   deployments without such resources get nothing extra today, while BYOC
   Container/Daemon (Horizon) will receive the receiver env through the identical
   snapshot path once those controllers consume it.

Manager: new free fn `commands_receiver_env_vars` called unconditionally in
`build_environment_variables` (section 3b, ungated), token = `deployment_token`.
Operator: `stack.receiver_command_env_vars(&commands_url, Some(&sync_config.token))`
alongside the worker helper, token = operator sync token (same reuse + security
TODO as the worker polling token).

## Per-constraint compliance map

| Constraint | Status | Notes |
|---|---|---|
| C2 five-var contract, per-resource scoped, keyed on command_targets() Container\|Daemon | DONE | `Stack::receiver_command_env_vars` emits URL (Plain), TOKEN (Secret, if present), TARGET_RESOURCE_TYPE (`container`/`daemon` via new `CommandTargetType::as_str`), TARGET_RESOURCE_ID — each `target_resources: [id]`. `ALIEN_DEPLOYMENT_ID` NOT re-emitted (already deployment-wide from manager/operator; verified it reaches Container/Daemon via inject_into_environment). |
| C2 tunables DEVIATION | RECORDED | LEASE_SECONDS/MAX_LEASES/POLL_*/DRAIN_TIMEOUT/TOKEN_FILE not injected — no consts exist and neither receiver twin reads them (fixed defaults). Deferred; call out in squash commit body. |
| C3 passthrough cut + honest grep audit | DONE | Container plan drops passthrough; both daemon `commands_enabled → add_passthrough_transport_env_vars` gates removed. Helpers NOT orphaned (build-pod controller still uses them) so kept. Audit table below. |
| C4 mechanism reuse + platform-gate w/ evidence | DONE | Sibling helper next to `worker_command_polling_env_vars`; manager/operator call it beside the worker helper; platform-gate decision documented in manager code + above. |
| C5 K8s token via secret machinery + local URL rewrite | DONE | Token is a Secret var → `applicable_secret_environment_variables` → `KubernetesEnvSecretPlan.keys` → `secretKeyRef`. Container already had this; **daemon K8s controller newly wired** to `reconcile_environment_secret` + secretKeyRef rendering + `env-secret-checksum` pod-roll annotation (create & update paths). Local: `ALIEN_COMMANDS_URL` added to container_manager's host.docker.internal rewrite list. |
| C6 full test matrix on survey harnesses | DONE | Counts below. |

## Passthrough / ALIEN_TRANSPORT audit (rg over crates/alien-infra + crates/alien-core)

Grep is NOT zero — every remaining hit is legitimate and assessed:

| Hit | Verdict |
|---|---|
| `runtime_environment.rs` `ENV_ALIEN_TRANSPORT` const + reserved-name coverage | KEEP — still a reserved runtime name; workers use it |
| `runtime_environment.rs` `worker_transport_runtime_environment_plan` (lambda/cloud-run/container-app/http/**passthrough** on Local/Test) | KEEP — Worker transport, a separate concern; untouched by design (guard test added) |
| `runtime_environment.rs` `passthrough_transport_runtime_environment_plan` helper | KEEP — not orphaned; used by build-pod controller |
| `container/environment_variables.rs` `add_passthrough_transport_env_vars` method + import | KEEP — not orphaned; used by build-pod controller |
| `build/kubernetes.rs:671` `.add_passthrough_transport_env_vars()` | KEEP — build JOB pods (not command receivers); out of ALIEN-222 scope (build territory) |
| `public_endpoint.rs:13` "TCP passthrough without TLS" | KEEP — unrelated network concept |
| `daemon/{kubernetes,local}.rs` + `runtime_environment.rs` comments/tests mentioning passthrough | KEEP — explanatory comments + passthrough-gone assertions |

No Container/Daemon **command-signal** passthrough remains. Runtime-side
`TransportType::Passthrough` deletion is explicitly ALIEN-223 follow-up
(alien-runtime, out of scope per C3 "do not touch alien-runtime"); recorded in
`.superpowers/sdd/progress.md`.

## Test counts (all green)

- alien-core lib: 377 passed (incl. 3 new receiver stack tests, `as_str` test, 2 passthrough-gone/worker-guard tests).
- alien-manager: 41 passed (2 new receiver tests: contract-scoping + empty-without-stack).
- alien-operator: 18 passed (1 new: receiver env scoped per Container/Daemon).
- alien-infra lib (`--features all-platforms`): 396 passed (1 new: token-via-secret-plan scoped per resource + worker-excluded).
- alien-local lib: 19 passed (1 new: localhost→host.docker.internal rewrite for ALIEN_COMMANDS_URL + polling URL, user/non-localhost untouched).
- alien-deployment: 126 passed (inject_into_environment Container/Daemon dispatch unaffected).

Gates: `cargo check --workspace` clean (pre-existing warnings only). `cargo clippy`
on touched crates: no NEW warnings from these changes (only pre-existing
large-Err-variant style). `cargo fmt` applied to touched crates.

## TS collateral

`git diff --name-only HEAD~3` contains zero `.ts/.tsx/.js/package.json/pnpm-lock.yaml`
files — Rust-only change. Root biome not run (fresh worktree has no node_modules;
missing dev CLI is machine setup, and there is no TS to lint).

## Deviations / notes for reviewer

- Tunable receiver vars intentionally not injected (C2 deviation, above).
- **Daemon K8s controller** gained real new wiring (env-secret reconcile +
  secretKeyRef render + checksum pod-roll) to match the container token path
  (C5 "DaemonSet env + secret ref"). Manifest-level K8s controller tests were NOT
  added: there is no existing K8s-client mock harness in-crate, so such a test
  would be heavy new infra asserting mostly k8s_openapi serialization; instead the
  load-bearing pure function (`applicable_secret_environment_variables`, which
  drives the secretKeyRef plan for both container and daemon) is unit-tested for
  per-resource scoping + worker exclusion.
- `crates/alien-local/src/store_probe.rs` (9 lines) is fmt-only collateral from
  `cargo fmt -p alien-local` fixing pre-existing drift in a crate I touched.
- Horizon Container/Daemon controllers (BYOC token as structured secret refs) are
  the private follow-up, per survey §3 — not in this worktree.
