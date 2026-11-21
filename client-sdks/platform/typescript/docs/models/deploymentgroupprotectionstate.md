# DeploymentGroupProtectionState

`on`: new deployments are protected. `waiting`: the switch is on but something in `missing` isn't there yet. `off`: the vendor turned it off.

## Example Usage

```typescript
import { DeploymentGroupProtectionState } from "@alienplatform/platform-api/models";

let value: DeploymentGroupProtectionState = "off";
```

## Values

```typescript
"on" | "waiting" | "off"
```