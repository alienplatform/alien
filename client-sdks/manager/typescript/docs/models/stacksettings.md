# StackSettings

User-customizable deployment settings specified at deploy time.

These settings are provided by the customer via CloudFormation parameters,
Terraform attributes, CLI flags, or Helm values. They customize how the
agent is deployed and what capabilities are enabled.

**Key distinction**: StackSettings is user-customizable, while ManagementConfig
is platform-derived (from the Agent Manager's ServiceAccount).

## Example Usage

```typescript
import { StackSettings } from "@alienplatform/manager-api/models";

let value: StackSettings = {};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `deploymentModel`                                                      | [models.DeploymentModel](../models/deploymentmodel.md)                 | :heavy_minus_sign:                                                     | Deployment model: how updates are delivered to the remote environment. |
| `domains`                                                              | [models.DomainSettings](../models/domainsettings.md)                   | :heavy_minus_sign:                                                     | N/A                                                                    |
| `heartbeats`                                                           | [models.HeartbeatsMode](../models/heartbeatsmode.md)                   | :heavy_minus_sign:                                                     | How heartbeat health checks are handled.                               |
| `network`                                                              | *models.NetworkSettings*                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `telemetry`                                                            | [models.TelemetryMode](../models/telemetrymode.md)                     | :heavy_minus_sign:                                                     | How telemetry (logs, metrics, traces) is handled.                      |
| `updates`                                                              | [models.UpdatesMode](../models/updatesmode.md)                         | :heavy_minus_sign:                                                     | How updates are delivered to the agent.                                |