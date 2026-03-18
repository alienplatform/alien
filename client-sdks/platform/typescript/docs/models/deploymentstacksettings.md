# DeploymentStackSettings

User-provided configuration (network, deployment model, approvals)

## Example Usage

```typescript
import { DeploymentStackSettings } from "@alienplatform/platform-api/models";

let value: DeploymentStackSettings = {};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `deploymentModel`                                                          | [models.DeploymentDeploymentModel](../models/deploymentdeploymentmodel.md) | :heavy_minus_sign:                                                         | Deployment model: how updates are delivered to the remote environment.     |
| `domains`                                                                  | *models.DeploymentDomainsUnion*                                            | :heavy_minus_sign:                                                         | N/A                                                                        |
| `heartbeats`                                                               | [models.DeploymentHeartbeats](../models/deploymentheartbeats.md)           | :heavy_minus_sign:                                                         | How heartbeat health checks are handled.                                   |
| `network`                                                                  | *models.DeploymentNetworkUnion*                                            | :heavy_minus_sign:                                                         | N/A                                                                        |
| `telemetry`                                                                | [models.DeploymentTelemetry](../models/deploymenttelemetry.md)             | :heavy_minus_sign:                                                         | How telemetry (logs, metrics, traces) is handled.                          |
| `updates`                                                                  | [models.DeploymentUpdates](../models/deploymentupdates.md)                 | :heavy_minus_sign:                                                         | How updates are delivered to the deployment.                               |