# AI Chatbot

A streaming chatbot that answers questions about a private Postgres by writing SQL and running it through a tool. The model is served by the deployment's own cloud and the database is reachable only from inside the stack, so there are no API keys and no database credentials in the app.

The app builds with the included Dockerfile (Next.js [standalone output](https://nextjs.org/docs/app/api-reference/config/next-config-js/output)) and runs as a single container behind an HTTPS load balancer.

## What's included

| Resource | Type | Description |
|----------|------|-------------|
| `app` | Container | The Next.js chat app, built from the Dockerfile and exposed over HTTP |
| `llm` | AI (live) | Model-less AI resource; the gateway serves it from the deployment's cloud |
| `db` | Postgres (live) | Private database, reachable only from same-stack workloads |

## How it works

- `alien.ts` links both resources to the container, so Alien grants the workload `ai/invoke` and `postgres/data-access` and injects `ALIEN_LLM_BINDING` and `ALIEN_DB_BINDING`.
- `app/api/chat/route.ts` resolves the model endpoint with `getAiConnection("llm")` and streams with the Vercel AI SDK. On a cloud the binding routes through Alien's embedded gateway, which injects the workload's ambient credential; on `alien dev` it carries your own provider key and the app calls the provider directly.
- The gateway forwards each model to its own upstream wire format instead of translating, so the route picks the client to match: Claude models get the Anthropic client, everything else the OpenAI-compatible one. Both take the same `baseURL` and the same binding.
- The `queryDatabase` tool runs one SQL statement per call, bounded by the database rather than by parsing the model's output: read-only sessions, a statement timeout, and a row limit applied in SQL. It reads the connection with `postgres("db").connection()`, which resolves the password at runtime under the workload's own identity.
- `app/api/models/route.ts` calls `ai("llm").getAvailableModels()`, so the picker lists only the models this cloud has enabled.
- **See the data** in the header opens a drawer over the chat with the demo tables, read through the same read-only pool, so an answer can be checked against the rows it came from.

## Local development

Bring your own provider key -- locally there is no cloud identity, so the SDK uses the key directly (a BYO-key binding) instead of the gateway:

```bash
OPENAI_API_KEY=sk-... alien dev
```

Open the printed URL and ask a data question, e.g. *"How many enterprise customers do we have and what's the total MRR?"* The model writes the SQL, calls `queryDatabase`, and summarizes the result. The demo tables are created and filled on the first question, so there is nothing to seed by hand.

## Deploying

```bash
alien deploy production --platform aws   # or gcp / azure
```

Alien builds the container image from the Dockerfile, pushes it, and provisions the compute, the database, and the load balancer. The deploy output prints the public URL.

That URL is open, so anyone who has it can ask questions and spend model quota. It is what makes the example something you can click and try, but a real deployment should put authentication and a per-caller rate limit in front of `/api/chat`.

## Model availability

`getAvailableModels()` returns what is enabled on your deployment's cloud right now. Open-weight models work out of the box; Claude needs a one-time activation first -- the Anthropic use-case form on AWS Bedrock, Model Garden on GCP Vertex, or Marketplace terms on Azure AI Foundry. Until then it simply does not appear in the picker, and every other model keeps working.

## Learn more

- [Postgres reference](https://alien.dev/docs/infrastructure/postgres)
- [Container reference](https://alien.dev/docs/infrastructure/container)
- [Stacks](https://alien.dev/docs/stacks)
