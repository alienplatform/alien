# SyncReconcileResponseStackSettings

User-customizable deployment settings specified at deploy time.

These settings are provided by the customer via CloudFormation parameters,
Terraform attributes, CLI flags, or Helm values. They customize how the
agent is deployed and what capabilities are enabled.

**Key distinction**: StackSettings is user-customizable, while ManagementConfig
is platform-derived (from the Agent Manager's ServiceAccount).

## Example Usage

```typescript
import { SyncReconcileResponseStackSettings } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseStackSettings = {};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `deploymentModel`                                                                                | [models.SyncReconcileResponseDeploymentModel](../models/syncreconcileresponsedeploymentmodel.md) | :heavy_minus_sign:                                                                               | Deployment model: how updates are delivered to the remote environment.                           |
| `domains`                                                                                        | *any*                                                                                            | :heavy_minus_sign:                                                                               | N/A                                                                                              |
| `heartbeats`                                                                                     | [models.SyncReconcileResponseHeartbeats](../models/syncreconcileresponseheartbeats.md)           | :heavy_minus_sign:                                                                               | How heartbeat health checks are handled.                                                         |
| `network`                                                                                        | *any*                                                                                            | :heavy_minus_sign:                                                                               | N/A                                                                                              |
| `telemetry`                                                                                      | [models.SyncReconcileResponseTelemetry](../models/syncreconcileresponsetelemetry.md)             | :heavy_minus_sign:                                                                               | How telemetry (logs, metrics, traces) is handled.                                                |
| `updates`                                                                                        | [models.SyncReconcileResponseUpdates](../models/syncreconcileresponseupdates.md)                 | :heavy_minus_sign:                                                                               | How updates are delivered to the agent.                                                          |