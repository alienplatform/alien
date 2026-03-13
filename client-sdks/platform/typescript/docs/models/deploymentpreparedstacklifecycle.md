# DeploymentPreparedStackLifecycle

Describes the lifecycle of a resource within a stack, determining how it's managed and deployed.

## Example Usage

```typescript
import { DeploymentPreparedStackLifecycle } from "@aliendotdev/platform-api/models";

let value: DeploymentPreparedStackLifecycle = "live-on-setup";
```

## Values

```typescript
"frozen" | "live" | "live-on-setup"
```