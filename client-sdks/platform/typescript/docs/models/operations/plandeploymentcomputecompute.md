# PlanDeploymentComputeCompute

Deployment-time compute choices for Alien-managed compute pools.

Application source declares portable pool requirements. This settings
object stores the concrete choices made for one deployment, such as the
provider machine type and selected machine counts.

## Example Usage

```typescript
import { PlanDeploymentComputeCompute } from "@alienplatform/platform-api/models/operations";

let value: PlanDeploymentComputeCompute = {};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `containers`                                                                                                             | Record<string, [operations.PlanDeploymentComputeContainers](../../models/operations/plandeploymentcomputecontainers.md)> | :heavy_minus_sign:                                                                                                       | Per-replica resources selected within each container's declared ranges.                                                  |
| `pools`                                                                                                                  | Record<string, *operations.PlanDeploymentComputePoolsUnion*>                                                             | :heavy_minus_sign:                                                                                                       | Selected compute choices keyed by pool ID.                                                                               |