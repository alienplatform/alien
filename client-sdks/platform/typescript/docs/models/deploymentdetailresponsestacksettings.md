# DeploymentDetailResponseStackSettings

User-provided configuration (network, deployment model, approvals)

## Example Usage

```typescript
import { DeploymentDetailResponseStackSettings } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseStackSettings = {};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `deploymentModel`                                                                                      | [models.DeploymentDetailResponseDeploymentModel](../models/deploymentdetailresponsedeploymentmodel.md) | :heavy_minus_sign:                                                                                     | Deployment model: how updates are delivered to the remote environment.                                 |
| `domains`                                                                                              | *any*                                                                                                  | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `heartbeats`                                                                                           | [models.DeploymentDetailResponseHeartbeats](../models/deploymentdetailresponseheartbeats.md)           | :heavy_minus_sign:                                                                                     | How heartbeat health checks are handled.                                                               |
| `network`                                                                                              | *any*                                                                                                  | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `telemetry`                                                                                            | [models.DeploymentDetailResponseTelemetry](../models/deploymentdetailresponsetelemetry.md)             | :heavy_minus_sign:                                                                                     | How telemetry (logs, metrics, traces) is handled.                                                      |
| `updates`                                                                                              | [models.DeploymentDetailResponseUpdates](../models/deploymentdetailresponseupdates.md)                 | :heavy_minus_sign:                                                                                     | How updates are delivered to the agent.                                                                |