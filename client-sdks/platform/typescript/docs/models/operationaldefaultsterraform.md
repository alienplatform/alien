# OperationalDefaultsTerraform

Operational settings fixed by the Standard configuration profile.

## Example Usage

```typescript
import { OperationalDefaultsTerraform } from "@alienplatform/platform-api/models";

let value: OperationalDefaultsTerraform = {
  heartbeats: "on",
  telemetry: "approval-required",
  updates: "auto",
};
```

## Fields

| Field                                                          | Type                                                           | Required                                                       | Description                                                    |
| -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- |
| `heartbeats`                                                   | [models.TerraformHeartbeats](../models/terraformheartbeats.md) | :heavy_check_mark:                                             | How heartbeat health checks are handled.                       |
| `telemetry`                                                    | [models.TerraformTelemetry](../models/terraformtelemetry.md)   | :heavy_check_mark:                                             | How telemetry (logs, metrics, traces) is handled.              |
| `updates`                                                      | [models.TerraformUpdates](../models/terraformupdates.md)       | :heavy_check_mark:                                             | How updates are delivered to the deployment.                   |