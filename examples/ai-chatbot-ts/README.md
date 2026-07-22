# AI chatbot

A streaming chatbot that runs as a single container in the customer's cloud and
answers questions about a private **Postgres** it queries with a tool. No API
keys and no database credentials in the app.

## How it works

- `alien.ts` declares a model-less `alien.AI("llm")` and a private
  `alien.Postgres("db")`, and links both to the container. At deploy time Alien
  grants the workload `ai/invoke` + `postgres/data-access` and injects
  `ALIEN_LLM_BINDING` and `ALIEN_DB_BINDING`.
- `app/api/chat/route.ts` resolves the model endpoint with
  `getAiConnection("llm")` and streams with the Vercel AI SDK's `streamText`.
  On a cloud, the binding routes through Alien's embedded OpenAI-compatible
  gateway, which injects the workload's ambient cloud credential. On `alien dev`
  the binding carries the developer's own provider key, and the app calls the
  provider directly.
- The chat route gives the model a `queryDatabase` tool (a single read-only
  SELECT against Postgres). It reads the connection with
  `getPostgresConnection("db")`, which resolves the password at runtime using
  the workload's own identity, so the password never sits in checked-in config.
- `app/api/models/route.ts` calls `ai("llm").getAvailableModels()` so the UI's
  model picker reflects the binding's model set.
- The UI (`app/page.tsx`) is a full chat surface built on `useChat`: streamed
  markdown answers, a card for every `queryDatabase` call showing the SQL and
  the rows it returned, suggested questions, a model picker, and stop/retry.

## Run it

In the customer's cloud:

```bash
alien deploy
```

Locally, bring your own provider key:

```bash
OPENAI_API_KEY=sk-... alien dev
```

Open the app URL, click **Seed demo data** (or `curl -X POST <app-url>/api/seed`),
and ask a data question, e.g. *"How many enterprise customers do we have and
what's the total MRR?"* The model writes the SQL, calls `queryDatabase`, and
summarizes the result.
