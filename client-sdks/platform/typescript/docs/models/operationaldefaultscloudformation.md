# OperationalDefaultsCloudformation

Operational settings fixed by the Standard configuration profile.

## Example Usage

```typescript
import { OperationalDefaultsCloudformation } from "@alienplatform/platform-api/models";

let value: OperationalDefaultsCloudformation = {
  heartbeats: "off",
  telemetry: "approval-required",
  updates: "auto",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `heartbeats`                                                             | [models.CloudformationHeartbeats](../models/cloudformationheartbeats.md) | :heavy_check_mark:                                                       | How heartbeat health checks are handled.                                 |
| `telemetry`                                                              | [models.CloudformationTelemetry](../models/cloudformationtelemetry.md)   | :heavy_check_mark:                                                       | How telemetry (logs, metrics, traces) is handled.                        |
| `updates`                                                                | [models.CloudformationUpdates](../models/cloudformationupdates.md)       | :heavy_check_mark:                                                       | How updates are delivered to the deployment.                             |