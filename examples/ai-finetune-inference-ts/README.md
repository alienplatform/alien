# AI Fine-tune + Inference

Fine-tune a base model **inside the customer's cloud**, then serve it for inference — the training data and the tuned weights never leave the customer's account. Extends the AI gateway (`alien.AI`) with a `.finetune()` declaration.

## What it shows

- One `alien.AI` resource that both **tunes** a base model and **serves** it (plus the base foundation models) through the same OpenAI-compatible gateway.
- Training data read from the customer's own object storage (S3 / GCS / Blob) under the workload's ambient identity — no keys, no data egress.
- The same app deploys unchanged to **AWS Bedrock**, **GCP Vertex AI**, and **Azure AI Foundry**; the resource resolves to the deploy-target's managed tuning + inference service.

## How it works

```
alien.Storage("dataset")          # S3 / GCS / Blob in the customer's account
        │  training.jsonl uploaded by the worker
        ▼
alien.AI("llm").finetune({...})   # on deploy, the controller submits the
        │                         # provider's tuning job reading `dataset`,
        │                         # polls to completion, records the artifact
        ▼
gateway serves "support-tuned"    # tuned model routed alongside the base catalog
```

At deploy time the AI resource's cloud controller submits the tuning job (Bedrock `CreateModelCustomizationJob`, Vertex `tuningJobs`, or Foundry `fine_tuning.jobs`), polls it to completion via its heartbeat loop, and records the tuned artifact. The gateway then routes the public id `support-tuned` to that artifact — so app code calls it exactly like a base model, only the `model` string differs.

## API

| Route | Method | Purpose |
|-------|--------|---------|
| `/dataset` | POST | Upload JSONL training data into the customer's bucket (body = JSONL) |
| `/finetune/status` | GET | `ready` once the tuning job has completed, else `pending` |
| `/chat` | POST | Inference against a base foundation model (`{ "message": "..." }`) |
| `/chat-tuned` | POST | Inference against the fine-tuned model — same call, different model id |

## Run locally

```bash
npm install
alien dev
```

Upload the sample dataset and query the tuned model:

```bash
curl -X POST --data-binary @sample-training.jsonl http://localhost:8080/dataset
curl http://localhost:8080/finetune/status
curl -X POST http://localhost:8080/chat-tuned -d '{"message":"How do I reset my password?"}'
```

On the **local** platform the AI resource is a BYO-key provider (set `OPENAI_API_KEY`); fine-tuning is a managed-cloud capability, so `/finetune/status` reports `pending` locally and the tuned route falls back to the base model. Deploy to a cloud to exercise the real tuning flow.

## Picking `baseModel` per cloud

`baseModel` is a **provider-native** id — set it to match the cloud you deploy to (see `alien.ts`):

| Cloud | Service | Example `baseModel` | Tuning method |
|-------|---------|--------------------|---------------|
| AWS | Bedrock | `amazon.nova-lite-v1:0` | SFT (`sft`) — also RFT on Nova |
| GCP | Vertex AI | a Gemini model id | Supervised (`sft`); Vertex does **not** expose LoRA/QLoRA as a user knob for Gemini |
| Azure | AI Foundry | a `gpt-4o` / `gpt-4.1` family id | `sft`, plus `dpo` on some models (LoRA underneath) |

## Data residency — read before you ship

"In the customer's cloud" is not automatic on every tier. What the verified provider docs say:

- **AWS Bedrock** — training data is S3-in / S3-out in the customer's buckets; the job runs under a customer IAM role; the custom model serves **on-demand** (Provisioned Throughput is *not* required — a common misconception). Private connectivity via PrivateLink. Region-confined by default.
- **Azure AI Foundry** — training data and the tuned model are stored at rest in the customer's Foundry resource, in-tenant, same geography (AES-256, optional CMK). **But**: the *Global Standard* and *Developer* deployment/training tiers may move weights outside the resource's region for cost — pin **Standard** if you need strict residency. Two gotchas: training JSONL must be UTF-8 **with a BOM**, and importing from Blob requires the storage account to allow **public** network access.
- **GCP Vertex AI** — supervised-tunes Gemini and auto-provisions a managed tuned-model endpoint; per-epoch checkpoints are auto-deployed. Residency follows the chosen region, but auto-deployed checkpoints can relax strict regional confinement — verify for your region.

For a hard "training data **and** tuned weights never leave the chosen region" guarantee, pin the region and the residency-preserving tier on each provider (Bedrock in-region + Standard-equivalent, Azure **Standard**, Vertex single-region), and confirm against current provider docs — these tiers and defaults change.

## Training data format

JSONL, one example per line, in the OpenAI chat/conversational shape:

```json
{"messages":[{"role":"user","content":"How do I reset my password?"},{"role":"assistant","content":"Go to Settings → Security → Reset password, then follow the emailed link."}]}
```

See `sample-training.jsonl`. Real fine-tuning needs enough examples to matter (Azure requires ≥ 10; providers recommend hundreds) — the sample is illustrative.

## License

ISC
