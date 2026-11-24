# Agent Sandboxes

An agent product that runs one sandbox per conversation inside each customer's cloud. The agent (Claude Code) works with the customer's internal API, but it never holds a real credential and it can reach nothing except the model and one connector the customer reviews. Every change it asks for waits until a person approves it.

The directories match the diagram: `control-plane/` is your SaaS, `data-plane/` is the Alien stack, and `customer-network/` stands in for what the customer already has.

The control plane is your SaaS: a chat UI and a thread router. It talks to each customer's deployment only through Alien commands, so it needs no network path into their cloud.

```
┌─ Your SaaS ──────────┐            ╔═ Customer's AWS account ═════════════════════════════════════╗
│                      │            ║                                                              ║
│ control-plane/       │            ║  ┌─ data-plane/ · the Alien stack ────────────────────────┐  ║
│                      │            ║  │                                                        │  ║
│ chat UI, threads     │  commands  ║  │  ┏━━━━━━━━━━┓             ┏━━━━━━━━━━━━━━━━━━━━━━━━━┓  │  ║
│ binds each turn to   │────────────╫──┼─▶┃ runner   ┃─ one turn ▶ ┃ Sandbox, per thread     ┃  │  ║
│ the user asking      │            ║  │  ┗━━━━━━━━━━┛             ┃ Claude Code             ┃  │  ║
│ approve / deny       │            ║  │                           ┃ no internet             ┃  │  ║
│                      │            ║  │                           ┃ placeholder keys        ┃  │  ║
│                      │            ║  │                           ┗━━━┯━━━━━━┯━━━━━━━━━━━━━━┛  │  ║
│                      │            ║  │  link:connector               │      │ link:model      │  ║
│                      │            ║  │  + turn token                 │      ▼                 │  ║
│                      │  commands  ║  │  ┏━━━━━━━━━━━━━┓              │  ┏━━━━━━━┓             │  ║
│                      │────────────╫──┼─▶┃ connector   ┃◀─────────────┘  ┃ model ┃──▶ Bedrock  │  ║
│                      │            ║  │  ┃ reads pass  ┃                 ┗━━━━━━━┛             │  ║
└──────────────────────┘            ║  │  ┃ writes wait ┃                                       │  ║
                                    ║  │  ┗━━━━━━┯━━━━━━┛                                       │  ║
                                    ║  └─────────┼──────────────────────────────────────────────┘  ║
                                    ║            │                                                 ║
                                    ║            ▼                                                 ║
                                    ║  ┌─ customer-network/ · already there ────────────────────┐  ║
                                    ║  │ internal API, private address                          │  ║
                                    ║  │ scanner, wants its own key                             │  ║
                                    ║  └────────────────────────────────────────────────────────┘  ║
                                    ╚══════════════════════════════════════════════════════════════╝
```

## What's in the stack

| Resource | Type | What it shows |
|----------|------|---------------|
| `agent` | Sandbox | One per thread. Its policy is `allow: ["link:model", "link:connector"]`: no internet, and its only ways out are two links. It starts with `ANTHROPIC_API_KEY=placeholder` and `CONNECTOR_TOKEN=placeholder`. |
| `runner` | Container, commands | Holds the sandbox binding. Creates, resumes and pauses each thread's sandbox and runs each agent turn in it. Not reachable from a sandbox. |
| `model` | Container | Serves the Anthropic Messages API to the sandboxes, backed by the stack's `llm` AI resource. On AWS that is Bedrock in the same account, called with the container's own identity, so no model key exists. |
| `connector` | Container, commands | The only way to the customer's private systems: the internal API and the scanner. No public endpoint; a sandbox reaches it only through its link. It checks the per-turn token on every request, lets reads through, holds writes until they are approved, and adds the customer's own scanner key to the scanner's requests. |
| `llm` | AI | The deployment's own model service. |

Outside the stack, the customer's network: [`customer-network/terraform/`](./customer-network/terraform) creates a VPC with private subnets behind a NAT gateway, plus the dedicated sandbox and gateway subnets the stack needs, a small orders API ([`customer-network/internal-api/`](./customer-network/internal-api)) and a scanner that lists findings ([`customer-network/scanner/`](./customer-network/scanner)), both on a private address. The stack is deployed into that VPC.

## Credentials the customer owns

The scanner wants an API key, and that key belongs to the customer. The stack declares it as a deployer secret (`scannerApiKey` in [`data-plane/alien.ts`](./data-plane/alien.ts)) that maps into the **connector's** environment only. The customer writes it into their own secret store (`alien dev vault set deployer-secrets scanner-api-key --stdin` locally, an SSM parameter on AWS; `alien deploy` prints the exact command). Alien, your control plane and the sandbox never see it, and the sandbox has no scanner key at all, not even a placeholder.

The connector is the one way to private systems: the sandbox calls `/scanner/findings` on its link, the connector checks the per-turn token (who may call), and sends the request to the scanner with `Authorization: Bearer <the customer's key>`. Scanner calls are reads, so they pass without approval. Environment values are read when the connector starts, so rotating the key means restarting it.

A public SaaS API (a cloud EDR or a ticketing API, say) goes the other way: the sandbox's egress policy allows that host, and the runner installs a host rule with `updateCredentials`, so the gateway adds the key on the way out. The key comes from a deployer secret mapped into the **runner's** environment, and the sandbox holds a placeholder:

```ts
await agent.updateCredentials(sandboxId, [
  { host: "api.example-tickets.com", header: "Authorization", valuePrefix: "Bearer ", value: process.env.TICKETS_API_KEY! },
])
// and in alien.ts: .defaultPolicy({ allow: ["link:model", "link:connector", "api.example-tickets.com"] })
```

This path is not demoed here: host rules reach public hosts only, never private addresses. OAuth refresh tokens and per-turn JWTs fit the same two paths: the connector refreshes or mints them and the runner installs the result as a rule. Both paths read their key at start.

## A turn, step by step

1. A message arrives for a thread. The router takes the thread's lock (open, turn start and pause of one thread run one at a time, so a thread gets one sandbox and a pause cannot catch a message half way) and sends `open` to the runner, which creates the thread's sandbox the first time, and resumes it if it was paused.
2. If the user changed since the last message, the runner first ends every process the previous user's turns started in the sandbox, and the thread shows how many. Claude Code itself is one process per turn and keeps its conversation on disk, so it needs nothing to stay running.
3. **The router binds the sandbox to the user who is asking** ([`bind-turn.ts`](./control-plane/src/bind-turn.ts)). The connector issues a token for that thread and user, and the runner installs it on the sandbox's connector link as a credential rule: the egress gateway sets `Authorization: Bearer <token>` on the sandbox's requests to the connector. The call returns after the gateway has the rule, and the turn starts only then. If the bind does not succeed the turn does not start and the thread shows the error. The new token replaces the thread's previous one, so the previous user's access ends at that point.
4. The runner starts one process for the turn: `claude -p --input-format stream-json --output-format stream-json --verbose`, with `--resume <session id>` after the first turn. It writes the user's message as one stream-json line and closes stdin; Claude Code answers, prints its `result` and exits.
5. The router long-polls the runner's `events` command and streams the agent's events to the browser over server-sent events.
6. When the agent runs `orders update`, the connector holds that HTTP request and lists it in its `approvals` command. The router shows it with Approve / Deny; `decide` answers the held request, and the agent sees the result as its command's output.
7. **When the turn ends, its access ends.** The runner removes the credential rule from the sandbox (rules apply to the whole sandbox, not to one process) and the connector revokes the turn's token. A process the agent left running in the background keeps running, but between turns it has no access through the connector: its next call is a 401. The next turn binds the sandbox again. Switch users and the previous user's background processes are ended before the new user is bound, so nothing runs as the wrong person.
8. A thread left idle for `IDLE_PAUSE_SECONDS` (default 120) is paused, if it is still idle when the pause gets the thread's lock. A message that arrives during a pause waits for it and then resumes the sandbox.

### Approvals

A held write can be approved only by someone who is in [`access.ts`](./data-plane/connector/src/access.ts) with the `orders:approve` right (`manager` in the example) and who is not the user who asked. The decision must name the thread the write belongs to. And the write must still belong to a live turn: if the token that asked is no longer the thread's current token (the turn ended, another user took over, or it expired), the decision is refused with "the request's turn ended; ask again", and the write is never forwarded. The check runs again right before the connector forwards an approved write.

The example has no sign-in, so the identity in the UI's *Approve as* box is a claim. In your product, your SaaS's own sign-in decides who is approving, and the control plane passes that identity to `decide`.

### Where the per-turn token travels

The connector creates the token and returns it over a command to your control plane, which hands it to the runner over another command; the runner gives it to the egress gateway. The control plane therefore sees each turn's token, and so does the command channel. It lives for the turn (revoked when the turn ends, with a time limit longer than the runner's turn timeout as a backstop). It never enters the sandbox, and the control plane does not store or log it.

## Run it locally

Requires Docker, [Bun](https://bun.sh), the `alien` CLI, `python3` (only for `demo.sh`) and, for the model, AWS credentials with Bedrock model access to the model you use (default `claude-sonnet-4-6`) in a region you choose.

```bash
pnpm install

# The sandbox image. Locally the stack uses it by tag; in the cloud it is built from data-plane/sandbox/.
docker build -t agent-sandboxes-sandbox:dev data-plane/sandbox/
```

`alien dev` stays in the foreground, so use one terminal for it and a second one for the rest.

**Terminal 1**, the data plane (`alien` commands run in `data-plane/`, where `alien.ts` is):

```bash
cd data-plane
SANDBOX_IMAGE=agent-sandboxes-sandbox:dev ALIEN_AI_LOCAL_BEDROCK_REGION=us-east-1 \
  alien dev --input internalApiUrl=http://internal-api:8080 --input scannerUrl=http://scanner:8081
```

`alien dev` prints the deployment id and the values for the control plane, then waits for the scanner's key.

**Terminal 2**:

```bash
# The customer's key for their scanner: written into the deployment's own secret store (from
# data-plane/, where `alien dev` keeps its state) and given to the mock scanner. Any value works;
# the two must match.
export SCANNER_API_KEY=$(openssl rand -hex 16)
(cd data-plane && printf '%s' "$SCANNER_API_KEY" | alien dev vault --deployment <deployment id> set deployer-secrets scanner-api-key --stdin)

# The internal API and the scanner, on the local deployment network, outside the stack.
docker compose -f customer-network/local/compose.yml up -d --build

# The control plane, with the values `alien dev` printed.
ALIEN_MANAGER_URL=<commands url> ALIEN_DEPLOYMENT_ID=<deployment id> ALIEN_TOKEN=<token> \
  pnpm control-plane
```

Open http://localhost:3000, start a thread and send a message.

`ALIEN_AI_LOCAL_BEDROCK_REGION` makes the local `llm` resource Bedrock in that region, called with your own AWS credentials: `alien dev` resolves your default chain (`AWS_PROFILE`, environment variables or SSO) when the `model` container starts and passes those keys to it. They can be long-lived, are visible inside that container and are not refreshed, so after an SSO session expires, run `aws sso login` and restart `alien dev`.

## Deploy to AWS

1. Create the customer's network and internal API:

   ```bash
   cd customer-network/terraform
   terraform init && terraform apply -var region=<region> -var scanner_api_key=<a key you choose>
   ```

   The scanner's key is stored in the account as an SSM parameter and read by the instance at boot (it is also in Terraform's state). The NAT gateway uses one Elastic IP and the stack adds its own; check the account's EIP and VPC quotas first. If the internal API's instance type has no capacity in the first two zones, set `instance_type` or `availability_zones`.

2. Release the stack:

   ```bash
   # The AWS build runs on arm64 and a sandbox is a live resource there.
   cd data-plane
   SANDBOX_LIFECYCLE=live alien release
   ```

3. Deploy it into the VPC. A sandbox with internet egress (this one reaches the model and the connector through the gateway) takes an existing VPC only through the CloudFormation or Terraform setup, not `alien deploy --network byo`. The setup asks for the VPC, its subnets, and two dedicated subnet sets: `sandbox_subnet_ids` (local routes only) and `sandbox_gateway_subnet_ids` (local routes and the NAT). The network Terraform created them and prints the values:

   ```bash
   # CloudFormation: the stack's parameters
   (cd ../customer-network/terraform && terraform output -raw cloudformation_parameters)
   # Terraform setup: the module's variables
   (cd ../customer-network/terraform && terraform output terraform_setup_values)
   ```

   For CloudFormation, find the package with `alien packages ls`, download the template with `alien packages download <package id> --artifact targets.aws -o template.yaml`, and upload it to an S3 bucket (it is larger than the 51 KB inline limit). Then create the stack with the printed parameters, plus the install `Token`, `ManagementExternalId` and `ManagingRoleArn` from the setup page, and the two stack inputs `InputInternalApiUrl` (`terraform output -raw internal_api_url`) and `InputScannerUrl` (`terraform output -raw scanner_url`):

   ```bash
   aws cloudformation create-stack --stack-name <stack> --template-url <s3 url> \
     --parameters file://params.json --capabilities CAPABILITY_NAMED_IAM
   ```

4. Write the scanner key into the deployment's secret store. It is an SSM parameter named `<stack>-deployer-secrets-scanner-api-key`:

   ```bash
   aws ssm put-parameter --name <stack>-deployer-secrets-scanner-api-key \
     --type SecureString --value file://key
   ```

5. Point the control plane at the deployment (`ALIEN_MANAGER_URL`, `ALIEN_DEPLOYMENT_ID`, `ALIEN_TOKEN`) and start it as above.


The model id is `claude-sonnet-4-6` by default; set `MODEL` when you release to use another model your account can invoke on Bedrock.

To remove everything: destroy the deployment, then `terraform destroy` in `customer-network/terraform/`.

## Demo walkthrough

Start a thread as `alice`. The **Run in sandbox** box runs a shell command in the thread's sandbox, outside the agent. The users are `alice` (reads and writes), `bob` (reads only) and `manager` (approves writes). *Approve as* picks who is approving.

1. **Only placeholder keys.** Run `env | sort`. The only credentials are `ANTHROPIC_API_KEY=placeholder` and `CONNECTOR_TOKEN=placeholder`. Ask the agent to print its environment: the same.
2. **Nothing else is reachable.** Run `curl -m5 https://example.com` and `curl -m5 http://<internal API private IP>:8080/orders`: both are refused. Then ask the agent to *list the orders*: it runs `orders list`, which goes through the connector and works.
3. **Writes wait for approval.** Ask: *refund order 1003*. The agent runs `orders update 1003 --status refunded`, says it is waiting, and an Approve / Deny bar appears. Approve as `manager` (alice cannot approve her own write), and the agent reports the refund.
4. **Switch the user mid-thread.** Pick `bob` and ask: *cancel order 1004*. The thread shows that alice's leftover processes were ended, and before the turn starts the sandbox is bound to `bob`; `bob` may read orders but not change them, so the connector answers 403 and the agent says so. Reads still work.
5. **A key the sandbox never holds.** Ask the agent: *list the scanner findings*. It runs `scanner findings`, the connector adds the customer's key, and three findings come back. Run `env | sort` in the sandbox again: there is no scanner key, and `curl http://<scanner private IP>:8081/findings` is refused.
6. **Idle threads pause and resume.** Ask the agent to *keep an orders watch running in the background*. It starts `orders watch` detached and reports its pid. Leave the thread idle until it shows *paused*, then send another message as the same user: it resumes, `pgrep -f "orders watch"` shows the same pid, and the agent remembers the earlier turns. Between turns the watcher has no credentials, so its calls get a 401 until the next turn binds the sandbox again. Local sandboxes do not pause, so this one shows on AWS.
7. **Another account.** Apply `customer-network/terraform/` and deploy the same release into a second AWS account. Everything above works the same.

### What you should see

Captured from a local run (demos 1-5; the sandbox's commands, not the agent's wording):

```
$ env | grep -E "KEY|TOKEN"
CONNECTOR_TOKEN=placeholder
ANTHROPIC_API_KEY=placeholder

$ curl -sS -m 8 https://example.com
curl: (56) CONNECT tunnel failed, response 403
$ curl -sS http://<internal API>:8080/orders          # refused by the gateway: [403]

$ orders list                                         # before the first bind
401: {"error":"no turn credentials on this request"}
$ orders list                                         # after the turn is bound to alice: the four orders
$ orders update 1001 --status shipped                 # held, listed for Approve / Deny
waiting for approval...                               # held until a manager decides
$ orders update 1004 --status cancelled               # after switching to bob
403: {"error":"bob may not change orders"}

$ scanner findings                                    # the connector adds the customer's key
F-201 web-01 high, F-202 web-02 medium, F-203 db-01 low
$ env | grep -ci scanner                              # in the sandbox
0
$ curl -sS http://<scanner>:8081/findings             # refused by the gateway: [403]
```

### Run the walkthrough as a script

[`demo.sh`](./demo.sh) makes the same calls the UI makes against the control plane's HTTP API and prints what comes back:

```bash
CONTROL_PLANE=http://localhost:3000 INTERNAL_API_IP=<private IP> SCANNER_IP=<private IP> ./demo.sh
```

Moments 1 and 2 need no model. Set `WITH_MODEL=0` to run only those; the rest go through the agent, approve the held write as `manager`, switch to `bob`, and wait `IDLE_WAIT` seconds (default 180) for the pause before resuming.

## Make it yours

- **Your internal systems:** replace `customer-network/internal-api/` and the routes in [`data-plane/connector/src/index.ts`](./data-plane/connector/src/index.ts), and give the agent its own tool in place of [`data-plane/sandbox/orders`](./data-plane/sandbox/orders).
- **Who may do what:** [`data-plane/connector/src/access.ts`](./data-plane/connector/src/access.ts) maps users to actions, including who may approve; read it from your own user store.
- **What needs approval:** the connector holds every write. Hold only some, or none.
- **The model:** `MODEL`, and any model the deployment's AI service offers.

Limits accepted to keep the example small: the connector keeps turn tokens and held writes in memory and runs one replica, so a connector restart fails the writes that were waiting and the next turn gets a new token. The control plane keeps threads in a JSON file and runs one process.

This example was proven in `alien dev`.

## Tests

```bash
pnpm test
```

The tests cover the connector's tokens (revoke, replace, expiry), its HTTP handler on a real server (a token is checked before routing, bob's write is a 403, a write is held and not forwarded until approved, an approval after a user switch or a turn's end is refused), the approver rules and the wrong-thread check, the router's order (bind before the turn, a failed bind stops the turn, the turn's access is cleared at its end, a user switch ends the old processes first, a pause racing a message, two first messages give one sandbox), the runner's per-thread lock, and the script that ends a user's processes, run against real processes.
