# DeploymentRegime

Supervisor regime reported on the last sync. Drives which `agent_target` payload (`binary` vs `helm`) the manager sends.

## Example Usage

```typescript
import { DeploymentRegime } from "@alienplatform/platform-api/models";

let value: DeploymentRegime = "kubernetes";
```

## Values

```typescript
"os-service" | "kubernetes"
```