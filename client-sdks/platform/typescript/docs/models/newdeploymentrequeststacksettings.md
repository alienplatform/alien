# NewDeploymentRequestStackSettings

Stack settings for deployment customization

## Example Usage

```typescript
import { NewDeploymentRequestStackSettings } from "@aliendotdev/platform-api/models";

let value: NewDeploymentRequestStackSettings = {};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `deploymentModel`                                                                              | [models.NewDeploymentRequestDeploymentModel](../models/newdeploymentrequestdeploymentmodel.md) | :heavy_minus_sign:                                                                             | Deployment model: how updates are delivered to the remote environment.                         |
| `domains`                                                                                      | *any*                                                                                          | :heavy_minus_sign:                                                                             | N/A                                                                                            |
| `heartbeats`                                                                                   | [models.NewDeploymentRequestHeartbeats](../models/newdeploymentrequestheartbeats.md)           | :heavy_minus_sign:                                                                             | How heartbeat health checks are handled.                                                       |
| `network`                                                                                      | *any*                                                                                          | :heavy_minus_sign:                                                                             | N/A                                                                                            |
| `telemetry`                                                                                    | [models.NewDeploymentRequestTelemetry](../models/newdeploymentrequesttelemetry.md)             | :heavy_minus_sign:                                                                             | How telemetry (logs, metrics, traces) is handled.                                              |
| `updates`                                                                                      | [models.NewDeploymentRequestUpdates](../models/newdeploymentrequestupdates.md)                 | :heavy_minus_sign:                                                                             | How updates are delivered to the agent.                                                        |