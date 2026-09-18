# OperationsPolicyRuleMaxRisk

For a wildcard pattern (`plugin/*` or `*`) only: the highest risk tier a WILDCARD ACCESS REQUEST against this pattern may cover. Null/absent means no wildcard grant above read-only is allowed for this pattern — write and destructive wildcards require an explicit rule setting this field. Ignored for exact `plugin/operation` rules (a named operation's risk is already its own declared tier) and for the auto/manual invoke decision itself.

## Example Usage

```typescript
import { OperationsPolicyRuleMaxRisk } from "@alienplatform/platform-api/models";

let value: OperationsPolicyRuleMaxRisk = "destructive";
```

## Values

```typescript
"read-only" | "mutating" | "destructive"
```