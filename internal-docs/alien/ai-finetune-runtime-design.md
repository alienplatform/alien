# AI fine-tuning: runtime, API-triggered design

Status: implementing. Supersedes the deploy-time tuning model.

## Why this changed

The first implementation submitted the cloud tuning job at **deploy time**, inside the
`Ai` resource controller, and blocked the resource in a `WaitingForTuningJob` state until
the job completed. Three problems made that wrong:

1. **Empty-bucket ordering.** The job wants training data at deploy time, but apps upload
   data at runtime (there is no data in the bucket on first deploy).
2. **Hours-long deploys.** Fine-tuning takes minutes to hours; a resource that isn't
   `Ready` until training finishes makes `alien release` block for hours.
3. **Retraining = redeploy.** Training on new data meant editing the resource and
   redeploying.

Fine-tuning is fundamentally an **imperative, long-running job**, not a declarative
resource state. So we move the trigger to runtime.

## The model

- `.finetune({...})` on the `Ai` resource is now a **capability declaration**: it says
  "this gateway may fine-tune `baseModel`, reading data from `trainingData`, serving the
  result as `servedModelId`", and it drives the `ai/finetune` permission grant. It does
  **not** start a job.
- The `Ai` controller reaches `Ready` immediately (no tuning states).
- At runtime the app calls the gateway to start/track jobs:
  - `ai("llm").finetune({ trainingKey? }) -> { jobId }` — submits the cloud tuning job.
  - `ai("llm").finetuneStatus(jobId) -> { status, model? }` — polls it.
- Inference against `servedModelId` works as soon as the tuned model is `Active`, via
  **rediscovery by convention** (below) — no state store.

## Gateway control-plane surface

New routes on the in-process gateway (alongside the inference proxy):

- `POST /<binding>/v1/finetune` — body `{ trainingKey?, baseModel?, method? }`. Submits
  the provider job using the binding's ambient credential and the `finetune` capability
  from the binding. Returns `{ jobId, servedModel }`.
- `GET  /<binding>/v1/finetune/<jobId>` — returns `{ status: "pending"|"running"|"succeeded"|"failed", model? }`.

The gateway already holds the ambient credential and signs arbitrary
host/service requests (`AmbientCred::authorize(req, service)`), so control-plane calls
(`bedrock` control host, Vertex `tuningJobs`, Foundry `fine_tuning.jobs`) reuse the same
credential path as inference. No new credential wiring.

### Per-cloud provider trait

```rust
#[async_trait]
trait FineTuneProvider {
    async fn submit(&self, spec: &FineTuneRequest) -> Result<JobHandle>;
    async fn status(&self, job: &str) -> Result<JobStatus>;        // by job id
    async fn resolve_served_model(&self, served_id: &str) -> Result<Option<String>>; // rediscovery
}
```

- **Bedrock**: submit = `CreateModelCustomizationJob`; status = `GetModelCustomizationJob`;
  rediscovery = `GetCustomModel(<deterministic-name>)` → if `modelStatus == Active`, its
  `modelArn` is the upstream id. `GetCustomModel` accepts the model **name**, so no ARN
  needs storing.
- **Vertex**: submit = `POST tuningJobs`; status = `GET tuningJobs/{id}`; rediscovery =
  the tuned endpoint id derived from the job / a list filtered by display name.
- **Foundry**: submit = `POST fine_tuning/jobs`; status = `GET fine_tuning/jobs/{id}`;
  on success the job carries `fine_tuned_model`; rediscovery = the deployment named after
  `servedModelId` (create-on-first-success), or `GET deployments/{name}`.

## Rediscovery by convention (stateless)

The gateway is per-process and stateless. A job started by one worker completes on the
cloud's side hours later, possibly after that worker is gone. So the gateway does **not**
track job state across restarts. Instead:

- The tuned model's cloud name is **deterministic** from the binding + `servedModelId`.
- On an inference request for `servedModelId`, if the route has no cached tuned upstream,
  the gateway calls `resolve_served_model(servedId)`; if the provider reports the model
  `Active`, it caches and routes to it. If not ready, it returns `model not available`.
- `GET /finetune/<jobId>` likewise queries the cloud live each call.

No storage dependency, no background poller. The trade-off: a completed job is only
"noticed" on the next status poll or inference request (fine — it's a pull model).

## Role / credentials note (fixes the deploy-time role-ARN bug)

The deploy-time version derived a job `roleArn` (`{prefix}-{id}`) that didn't reliably
exist. In the runtime model the gateway submits the job under the **workload's ambient
identity** — the same identity it uses for inference. Bedrock's `CreateModelCustomizationJob`
still needs a `roleArn` it can assume to read S3 / write output, so the gateway resolves
the workload's actual execution role (or a dedicated finetune role the `ai/finetune`
permission set provisions with a `bedrock.amazonaws.com` trust policy). This is resolved
at submit time from the binding, not guessed from a naming convention.

## Layers touched

- `alien-gateway`: new `finetune` module (trait + 3 providers), 2 routes, tuned-model
  cache + rediscovery in the router.
- `packages/ai-gateway` (SDK): `finetune()` / `finetuneStatus()` on the `ai()` client.
- `alien-core`: binding carries the `finetune` capability (baseModel, trainingData bucket,
  servedModelId, method) so the gateway can submit without a control-plane round-trip to
  the resource. `FinetuneSpec` stays on the resource as the declaration.
- `alien-infra` controllers: drop `SubmittingTuningJob`/`WaitingForTuningJob`; reach
  `Ready` immediately; still emit the `ai/finetune` grant and the capability in the binding.
- Example `ai-finetune-inference-ts`: `POST /dataset` → `POST /finetune` → poll
  `/finetune/status` → `POST /chat-tuned`.
