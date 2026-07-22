# AI quickstart (TypeScript)

The smallest possible Alien AI setup: one worker, one `AI` resource, no database.
The worker asks a question and the embedded gateway forwards it to a model served
by the deployment's own cloud — Bedrock on AWS, Vertex on GCP, Azure AI Foundry on
Azure — under the workload's ambient identity. No API keys anywhere.

## Run it locally

```bash
OPENAI_API_KEY=sk-... alien dev
```

Locally there is no cloud identity, so the SDK uses your key directly (a
BYO-key binding) instead of the gateway.

## Try it

```bash
curl "$URL/models"
curl "$URL/ask?q=Reply+with+exactly+one+word:+pong"
```

`/models` lists the curated models for the cloud you deployed to; `/ask` picks
the first one unless you pass `?model=`.
