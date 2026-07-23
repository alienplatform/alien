// EXAMPLE: Fine-tune a base model in the customer's cloud, then serve it for
// inference — without the training data or the tuned weights ever leaving the
// customer's account.

import * as alien from "@alienplatform/core"

// The training dataset lives in the customer's object storage:
// S3 on AWS, Cloud Storage on GCP, Blob Storage on Azure. The worker uploads
// JSONL examples here; the tuning job reads them in-account.
//
// NOTE: S3 bucket names are GLOBALLY unique across all AWS accounts. The bucket
// is named `<deployment-prefix>-<storage-id>`, so a generic id like "dataset"
// can collide with a bucket someone else already owns. Keep this id distinctive
// (and change it if you hit "bucket name is not available").
const dataset = new alien.Storage("finetune-training-data").build()

// A model-less AI gateway with a fine-tuning CAPABILITY. `.finetune(...)` here is a
// declaration, not a deploy-time trigger: the resource provisions and is Ready
// immediately (no job runs at deploy). The app starts a tuning job at RUNTIME by
// calling `ai("llm").finetune(...)` (see src/index.ts) — the gateway then submits the
// provider's job (Bedrock CreateModelCustomizationJob, Vertex tuningJobs, or Foundry
// fine_tuning.jobs) reading `dataset` in the customer's account, and serves the tuned
// model under `servedModelId` once it completes. Base models remain callable too.
//
// `baseModel` is a provider-native id; pick the one that matches the cloud you
// deploy to (see the README's per-provider table). The default here targets
// AWS Bedrock (Amazon Nova).
const llm = new alien.AI("llm")
  .finetune({
    baseModel: "amazon.nova-lite-v1:0",
    trainingData: dataset,
    trainingKey: "training.jsonl",
    servedModelId: "support-tuned",
    method: "sft",
  })
  .build()

const api = new alien.Worker("api")
  .code({ type: "source", src: "./", toolchain: { type: "typescript" } })
  // GCP Cloud Run gen2 requires >= 512 MiB; the default 256 MiB fails its preflight.
  .memoryMb(512)
  .publicEndpoint("api")
  // Linking injects the dataset binding and the AI gateway (ALIEN_LLM_BINDING).
  .link(dataset)
  .link(llm)
  .permissions("execution")
  .build()

export default new alien.Stack("ai-finetune-inference")
  .platforms(["aws", "gcp", "azure"])
  .add(dataset, "live")
  .add(llm, "live")
  .add(api, "live")
  .permissions({
    profiles: {
      execution: {
        // Read/write the dataset bucket, and both submit the tuning job and
        // invoke models (base + tuned) through the gateway.
        "finetune-training-data": ["storage/data-read", "storage/data-write"],
        "*": ["ai/invoke", "ai/finetune"],
      },
    },
  })
  .build()
