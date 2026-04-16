# SyncAcquireResponseStackSettings

User-customizable deployment settings specified at deploy time.

These settings are provided by the customer via CloudFormation parameters,
Terraform attributes, CLI flags, or Helm values. They customize how the
deployment runs and what capabilities are enabled.

**Key distinction**: StackSettings is user-customizable, while ManagementConfig
is platform-derived (from the Manager's ServiceAccount).

## Example Usage

```typescript
import { SyncAcquireResponseStackSettings } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseStackSettings = {};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `deploymentModel`                                                                            | [models.SyncAcquireResponseDeploymentModel](../models/syncacquireresponsedeploymentmodel.md) | :heavy_minus_sign:                                                                           | Deployment model: how updates are delivered to the remote environment.                       |
| `domains`                                                                                    | *models.SyncAcquireResponseDomainsUnion*                                                     | :heavy_minus_sign:                                                                           | N/A                                                                                          |
| `heartbeats`                                                                                 | [models.SyncAcquireResponseHeartbeats](../models/syncacquireresponseheartbeats.md)           | :heavy_minus_sign:                                                                           | How heartbeat health checks are handled.                                                     |
| `network`                                                                                    | *models.SyncAcquireResponseNetworkUnion*                                                     | :heavy_minus_sign:                                                                           | N/A                                                                                          |
| `telemetry`                                                                                  | [models.SyncAcquireResponseTelemetry](../models/syncacquireresponsetelemetry.md)             | :heavy_minus_sign:                                                                           | How telemetry (logs, metrics, traces) is handled.                                            |
| `updates`                                                                                    | [models.SyncAcquireResponseUpdates](../models/syncacquireresponseupdates.md)                 | :heavy_minus_sign:                                                                           | How updates are delivered to the deployment.                                                 |