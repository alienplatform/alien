# AccessApprover

Who approves access requests. `customer`: the customer approves on the Slack card (through the Alien app or your own Slack app) or, on Kubernetes, with kubectl. Slack cannot prove who a customer is, so the Slack click is approval in Slack through your control plane, not a verified customer identity. `team`: your own team approves in the Alien Slack app or the dashboard. Every group defaults to `customer`, and only a workspace admin changes it.

## Example Usage

```typescript
import { AccessApprover } from "@alienplatform/platform-api/models";

let value: AccessApprover = "team";
```

## Values

```typescript
"customer" | "team"
```