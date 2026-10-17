# PackageOperationalDefaultsTargets

Operational settings fixed by the Standard configuration profile.

## Example Usage

```typescript
import { PackageOperationalDefaultsTargets } from "@alienplatform/platform-api/models";

let value: PackageOperationalDefaultsTargets = {
  heartbeats: "on",
  telemetry: "off",
  updates: "approval-required",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `heartbeats`                                                             | [models.PackageTargetsHeartbeats](../models/packagetargetsheartbeats.md) | :heavy_check_mark:                                                       | How heartbeat health checks are handled.                                 |
| `telemetry`                                                              | [models.PackageTargetsTelemetry](../models/packagetargetstelemetry.md)   | :heavy_check_mark:                                                       | How telemetry (logs, metrics, traces) is handled.                        |
| `updates`                                                                | [models.PackageTargetsUpdates](../models/packagetargetsupdates.md)       | :heavy_check_mark:                                                       | How updates are delivered to the deployment.                             |