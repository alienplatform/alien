# Agent Sandboxes

An agent product that runs one sandbox per conversation inside each customer's cloud. The agent (Claude Code) works with the customer's internal API, but it never holds a real credential and it can reach nothing except the model and one connector the customer reviews. Every change it asks for waits until a person approves it.

The control plane is your SaaS: a chat UI and a thread router. It talks to each customer's deployment only through Alien commands, so it needs no network path into their cloud.

```
  Your SaaS                     Customer's AWS account (their VPC)
 ┌────────────────┐            ┌──────────────────────────────────────────────────────────┐
 │ control-plane/ │  commands  │  runner ─(sandbox binding)─▶ agent sandbox, per thread   │
 │  chat UI       ├───────────▶│                               Claude Code                │
 │  thread router │            │                               placeholder keys           │
 │  bindTurn()    │  commands  │                                     │                    │
 │  Approve/Deny  ├───────────▶│  connector                    egress gateway ─╳ internet │
 └────────────────┘            │   ▲   │                        │        │                │
                               │   │   │      link:connector    │        │ link:model     │
                               │   └───┼──── + turn token ──────┘        ▼                │
                               │       │                               model ──▶ Bedrock  │
                               │       ▼                                                  │
                               │  internal API (private address, outside the stack)       │
                               └──────────────────────────────────────────────────────────┘
```

## What's in the stack

| Resource | Type | What it shows |
|----------|------|---------------|
| `agent` | Sandbox | One per thread. Its policy is `allow: ["link:model", "link:connector"]`: no internet, and its only ways out are two links. It starts with `ANTHROPIC_API_KEY=placeholder` and `CONNECTOR_TOKEN=placeholder`. |
| `runner` | Container, commands | Holds the sandbox binding. Creates, resumes and pauses each thread's sandbox and runs each agent turn in it. Not reachable from a sandbox. |
| `model` | Container | Serves the Anthropic Messages API to the sandboxes, backed by the stack's `llm` AI resource. On AWS that is Bedrock in the same account, called with the container's own identity, so no model key exists. |
| `connector` | Container, commands | The only way to the internal API. No public endpoint; a sandbox reaches it only through its link. It checks the per-turn token on every request, lets reads through and holds writes until they are approved. |
| `llm` | AI | The deployment's own model service. |

Outside the stack, the customer's network: [`infra/`](./infra) creates a VPC with private subnets behind a NAT gateway and a small orders API ([`internal-api/`](./internal-api)) on a private address. The stack is deployed into that VPC.

## A turn, step by step

1. A message arrives for a thread. The router sends `open` to the runner, which creates the thread's sandbox the first time, and resumes it if it was paused.
2. **The router binds the sandbox to the user who is asking** ([`bind-turn.ts`](./control-plane/src/bind-turn.ts)). The connector issues a token for that thread and user, and the runner installs it on the sandbox's connector link as a credential rule: the egress gateway sets `Authorization: Bearer <token>` on the sandbox's requests to the connector. The call returns after the gateway has the rule, and the turn starts only then. The new token replaces the thread's previous one, so the previous user's access ends at that point.
3. The runner starts one process for the turn: `claude -p --input-format stream-json --output-format stream-json --verbose`, with `--resume <session id>` after the first turn. It writes the user's message as one stream-json line and closes stdin; Claude Code answers, prints its `result` and exits. The conversation stays on the sandbox's disk, so nothing runs between turns.
4. The router long-polls the runner's `events` command and streams the agent's events to the browser over server-sent events.
5. When the agent runs `orders update`, the connector holds that HTTP request and lists it in its `approvals` command. The router shows it with Approve / Deny; `decide` answers the held request, and the agent sees the result as its command's output.
6. After the turn, a thread left idle for `IDLE_PAUSE_SECONDS` (default 120) is paused. The next message resumes it.

## Run it locally

Requires Docker.

```bash
pnpm install

# The sandbox image. Locally the stack uses it by tag; in the cloud it is built from sandbox/.
docker build -t agent-sandboxes-sandbox:dev sandbox/

# The data plane.
SANDBOX_IMAGE=agent-sandboxes-sandbox:dev OPENAI_API_KEY=... \
  alien dev --input internalApiUrl=http://internal-api:8080

# The internal API, on the local deployment network, outside the stack.
docker compose -f infra/local/compose.yml up -d --build

# The control plane, with the values `alien dev` prints.
ALIEN_MANAGER_URL=<commands url> ALIEN_DEPLOYMENT_ID=<deployment id> ALIEN_TOKEN=<token> \
  pnpm control-plane
```

Open http://localhost:3000, start a thread and send a message.

## Deploy to AWS

1. Create the customer's network and internal API:

   ```bash
   cd infra
   terraform init && terraform apply
   terraform output
   ```

   The NAT gateway uses one Elastic IP and the stack adds its own; check the account's EIP and VPC quotas first.

2. Release the stack and deploy it into that VPC with `byo-vpc-aws`, passing the internal API's address:

   ```bash
   alien release
   alien-deploy deploy --token <deployment group token> --platform aws \
     --network byo --vpc-id <vpc_id> \
     --public-subnet-ids <public_subnet_ids> --private-subnet-ids <private_subnet_ids> \
     --input internalApiUrl=<internal_api_url>
   ```

3. Point the control plane at the deployment (`ALIEN_MANAGER_URL`, `ALIEN_DEPLOYMENT_ID`, `ALIEN_TOKEN`) and start it as above.

The model id is `claude-sonnet-4-6` by default; set `MODEL` when you release to use another model your account can invoke on Bedrock.

To remove everything: destroy the deployment, then `terraform destroy` in `infra/`.

## Demo walkthrough

Start a thread as `alice`. The **Run in sandbox** box runs a shell command in the thread's sandbox, outside the agent.

1. **Only placeholder keys.** Run `env | sort`. The only credentials are `ANTHROPIC_API_KEY=placeholder` and `CONNECTOR_TOKEN=placeholder`. Ask the agent to print its environment: the same.
2. **Nothing else is reachable.** Run `curl -m5 https://example.com` and `curl -m5 http://<internal API private IP>:8080/orders`: both are refused. Then ask the agent to *list the orders*: it runs `orders list`, which goes through the connector and works.
3. **Writes wait for approval.** Ask: *refund order 1003*. The agent runs `orders update 1003 --status refunded`, says it is waiting, and an Approve / Deny bar appears. Approve, and the agent reports the refund.
4. **Switch the user mid-thread.** Pick `bob` and ask: *cancel order 1004*. Before the turn starts, the sandbox is bound to `bob`; `bob` may read orders but not change them, so the connector answers 403 and the agent says so. Reads still work.
5. **Idle threads pause and resume.** Ask the agent to *keep an orders watch running in the background*. It starts `orders watch` detached and reports its pid. Leave the thread idle until it shows *paused*, then send another message: it resumes, `pgrep -f "orders watch"` shows the same pid, and the agent remembers the earlier turns. Local sandboxes do not pause, so this one shows on AWS.
6. **Another account.** Apply `infra/` and deploy the same release into a second AWS account. Everything above works the same.

## Make it yours

- **Your internal systems:** replace `internal-api/` and the routes in [`services/connector/src/index.ts`](./services/connector/src/index.ts), and give the agent its own tool in place of [`sandbox/orders`](./sandbox/orders).
- **Who may do what:** [`services/connector/src/access.ts`](./services/connector/src/access.ts) maps users to actions; read it from your own user store.
- **What needs approval:** the connector holds every write. Hold only some, or none.
- **The model:** `MODEL`, and any model the deployment's AI service offers.

Limits accepted to keep the example small: the connector keeps turn tokens and held writes in memory and runs one replica, so a connector restart fails the writes that were waiting and the next turn gets a new token. The control plane keeps threads in a JSON file and runs one process.

The example was proven on AWS and `alien dev`.

## Tests

```bash
pnpm test
```

The tests cover the connector's tokens, access and approvals, and the router's order: the sandbox is bound to the asking user before each turn starts, a refused bind keeps the turn from starting, and an idle thread pauses and resumes.
