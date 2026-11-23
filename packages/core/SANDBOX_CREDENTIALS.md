# Sandbox credentials

A sandbox often needs to call APIs as the user it is working for: a GitHub token, an OAuth
token your app refreshes, a short-lived JWT your app mints for calls back to its own API. The
sandbox should never hold those values. With credential rules it holds a placeholder, and the
deployment's egress gateway puts the real value on the request on the way out.

Credential rules need an egress gateway that serves admin protocol 2. A release whose gateway
image predates it refuses the call with a clear message (never a silent drop), and `alien dev`
keeps its earlier behavior: only sessions with links run behind the gateway. The release that
installs the newer gateway enables the feature.

```ts
import { sandbox } from "@alienplatform/bindings"

const agents = sandbox("agents")

// Every turn, from your app (never from inside the sandbox):
await agents.updateCredentials(sandboxId, [
  { host: "api.github.com", header: "Authorization", valuePrefix: "Bearer ", value: githubToken },
  { link: "connector", header: "Authorization", valuePrefix: "Bearer ", value: turnJwt },
])
```

```python
from alienplatform import sandbox

agents = sandbox("agents")
await agents.update_credentials(sandbox_id, [
    {"host": "api.github.com", "header": "Authorization", "valuePrefix": "Bearer ", "value": github_token},
    {"link": "connector", "header": "Authorization", "valuePrefix": "Bearer ", "value": turn_jwt},
])
```

Rust: `Sandbox::update_credentials(&self, sandbox_id, Vec<SandboxCredentialRule>)`.

## The placeholder pattern

Give the sandbox a placeholder where it expects a key, for example `GITHUB_TOKEN=placeholder`
in its environment, so tools that refuse to start without one still start. The tool sends
`Authorization: Bearer placeholder`; the gateway replaces the whole header with the rule's
value on requests that match the rule. Requests that match no rule go out as the sandbox sent
them, so a placeholder reaching an API is a 401, never a leak.

## A rule

| Field | Meaning |
|---|---|
| `host` | Exact lowercase DNS name of an internet host. No wildcard, no IP address. |
| `link` | Id of a service the stack links to this sandbox (see [sandbox links](./SANDBOX_LINKS.md)). |
| `port` | Default 443 for a host; every declared port for a link. |
| `method` | Uppercase HTTP method. Default: any. |
| `path` | Exact path, or a subtree when it ends in `/`. Default `/` (every path). |
| `header` | The header the gateway sets. It replaces every value of that header the sandbox sent. |
| `valuePrefix` | Prepended to the value, such as `Bearer `. |
| `value` | The real value, at most 8 KiB with its prefix. |

Exactly one of `host` and `link`. At most 32 rules per sandbox. A rule cannot set `Host`,
`Content-Length`, `Transfer-Encoding`, `Connection`, `Proxy-Authorization`, `Upgrade`,
`Trailer` or `TE`. When two rules set the same header on one request, the first wins.

## Guarantees

- **Replace, not merge.** The list is the sandbox's whole set. An empty list removes every
  rule. A removed rule stops applying to new requests at once.
- **Acknowledged.** The call returns only when every gateway replica holds the new set, the
  same guarantee `updateEgress` gives. Every request the sandbox sends after it returns uses
  the new set; a request already being forwarded keeps the set it started with. A connection
  carries one request, so nothing older lingers. If a replica is away the call fails with a
  retryable error; calling it again is safe, as the newest set wins. A replica that is still
  starting is brought up to date before it serves any request. Use one writer per sandbox:
  "newest" is decided by the writers' clocks, so if two app processes send sets for the same
  sandbox and their clocks differ, the set from the process whose clock is behind is
  acknowledged but not applied.
- **Per sandbox.** Rules apply only to the sandbox they were set for, identified by its own
  gateway identity. Another sandbox of the same Sandbox resource gets nothing.
- **Within the egress policy.** The policy is checked first. A rule for a destination the
  policy refuses has no effect: a credential never widens what a sandbox reaches.
- **Values stay out of reach.** Values live only in the gateway's memory and travel only over
  its authenticated admin and replica channels inside the deployment. They are never in logs,
  the connection log, metrics, error messages, stack state or the deployment's configuration,
  and never inside the sandbox (environment, files, or the CA).
- **Replica restarts.** A gateway replica that restarts takes the rules from a running peer
  before it serves again. If every replica is lost at once (a single-replica gateway that
  restarts, or a full outage), the rules are gone until your app calls `updateCredentials`
  again; until then requests go out with the sandbox's placeholders. Calling it every turn, as
  above, covers this.
- **Join window.** A replica that has just started (or been added) serves sandbox traffic
  with the egress policy as usual, but injects no credentials for the first few seconds (about
  15) until it has synced once after the other replicas can see it. Requests in that time go
  out with the sandbox's placeholders and the upstream answers 401. This keeps a rule you
  just removed from being applied by a replica that missed the removal.

## HTTPS and the sandbox CA

A rule for an internet `host` applies to HTTPS. When the sandbox opens a connection to a host
that has a rule for it, the gateway terminates TLS with a certificate from the sandbox's own
CA, puts the header on, and opens a verified TLS connection to the real host. Connections to
hosts without a rule pass through untouched. A plain-HTTP request to a host with a rule is
refused, so a value never crosses the internet unencrypted.

Each sandbox gets its own CA when it is created. It stays the same for the life of the sandbox,
whatever rules come and go, and covers DNS names only, never IP addresses. The gateway issues a
certificate only for a host that has a current rule for that sandbox. The CA's key never leaves
the gateway.

The sandbox trusts the CA through the usual variables, set for every command:

| Client | Reads |
|---|---|
| OpenSSL, curl, Go, Python `ssl` / `httpx` / `urllib` | `SSL_CERT_FILE`, `CURL_CA_BUNDLE` (the system roots plus the CA) |
| Python `requests` | `REQUESTS_CA_BUNDLE` |
| Node.js, and tools built on it such as Claude Code | `NODE_EXTRA_CA_CERTS`, read when the process starts |

Clients with their own trust store (Java's keystore, binaries with built-in roots, pinned
clients) must import the CA themselves; the file is the one `SSL_CERT_FILE` names.

## Links

A rule for a `link` applies to plain HTTP to that linked service, such as
`http://connector.svc:8080`; no TLS and no CA are involved, and the traffic stays inside the
deployment. HTTPS to a link with a rule is refused.

## What a rule cannot prevent

The gateway never follows redirects, matches the exact host the request names (with the same
`Host` header, TLS server name and connection target), and refuses ambiguous paths such as
`/v1/../admin` while a rule covers the destination. It cannot stop the destination itself from
sending a credential back: an echo endpoint, a debug page or an error body that repeats request
headers hands the value to the sandbox. Only write rules for APIs you trust, and narrow them
with `method` and `path`.

## Platforms

Every platform where the egress gateway runs: AWS, GCP, Azure, Kubernetes and local
development. A sandbox without the gateway (egress `none`) refuses `updateCredentials`. A
gateway older than this release refuses it too, with a message to re-run setup; it never
accepts rules it cannot apply.

On Azure the agent installs the CA the same way when it starts, and the binding records at create that it was handed over (Azure has no agent endpoint to ask later). A `host` rule is refused for a sandbox that was not given the CA.

On GCP the agent in the sandbox forwards traffic to the gateway, and it installs the CA when
the sandbox is created: it writes the trust bundle (the image's roots plus the CA) and every
command gets the same trust variables as elsewhere. A sandbox whose agent image predates this
refuses a `host` rule with a message that says so (`link` rules keep working, since they need no
CA); create the sandbox again with the agent image of the current release.

## Refusals

`updateCredentials` fails loudly when it cannot give the guarantees above. A refusal before any gateway is contacted (the first rows) changes nothing; a failure while writing can leave the new list on some replicas, so retry with the whole intended list:

| Situation | Message says |
|---|---|
| The deployment's gateway image predates credentials | per-sandbox credentials are not available with the egress gateway image this release installs; a later release enables them |
| The installed gateway is older than this release needs | the egress gateway does not serve per-sandbox credentials yet; deploy a release that installs a newer egress gateway and re-run setup |
| A replica did not acknowledge | a retryable error; the replicas that already took the list keep it, so call again with the same list (the list replaces, so repeating is safe) |
| Azure, `host` rule, sandbox whose agent was not given the CA | this sandbox was created without the gateway's CA; create it again (`link` rules work) |
| GCP, `host` rule, agent image without the CA install | this sandbox's agent predates trust-bundle delivery |
| AWS or Kubernetes, `host` rule, sandbox created before the gateway served credentials (no CA) | this sandbox was created without the gateway's CA; create it again (`link` rules work) |
| The sandbox has no egress policy (egress `none`) | the sandbox has no gateway enforcement |

## Limits we accepted

- Rules live only in gateway memory. After a gateway restart a replica catches up from its peers;
  if every replica restarted, send the list again (calling it each turn already does).
- A successful call means every replica holds the list. A failed call may be partly applied: the gateway applies it on the replica that received it before the others acknowledge. Retry with the whole intended list.
- When a list changes, the sandbox's open tunnels that the gateway relays without reading (a connection opened before a rule existed) are closed before the call returns; clients reconnect, and every request after the call carries the rules.
- A `host` rule is HTTPS only, a `link` rule is plain HTTP only. A client with its own trust
  store needs the CA imported (see above).
- A destination that echoes request headers can show the value to the sandbox.
- Only the CA is delivered into the sandbox, never a key; rules cannot widen the egress policy.

### Local development

In `alien dev` every sandbox whose egress is not `none` runs behind its own gateway, with or
without links, so policies and credential rules behave as they do in the cloud. The CA and the
trust bundle are mounted read-only at `/etc/alien-egress` in each session. Each running session
has its own Docker network, so the daemon's address pools bound how many sessions run at once
(about 30 with Docker's defaults; widen `default-address-pools` for more). Declared preview ports are
published on `127.0.0.1` by a small relay container beside each session. It sits on the
session's network, only forwards inbound connections to that one port, and gives the session no
route out; it is removed with the session.
