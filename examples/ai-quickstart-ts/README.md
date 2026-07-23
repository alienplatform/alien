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

`/models` lists only the models this cloud actually has enabled, each with an `id`,
`provider`, and `displayName`, so you can use it as a picker. `/ask` uses `?model=`,
or the first available model.

## Model availability

`getAvailableModels()` returns what is enabled on your deployment's cloud right now.
Some models need a one-time activation in the cloud provider before they appear:

- **AWS Bedrock**: open-weight models (GPT-OSS, Llama, Mistral, Qwen, and more) work
  out of the box. Claude needs the one-time Anthropic use-case form in the Bedrock
  console.
- **GCP Vertex**: Gemini works out of the box. Claude needs enabling in Vertex AI
  Model Garden with Anthropic's terms accepted (Google Cloud console).
- **Azure AI Foundry**: the GPT models are deployed for you. Claude needs a one-time
  Marketplace-terms acceptance and deployment in the Foundry portal.

Until you complete a model's activation step it simply will not appear in
`getAvailableModels()`. Your deployment still succeeds and every other model keeps
working.
