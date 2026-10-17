# PackageOperationalDefaultsModules

Operational settings fixed by the Standard configuration profile.

## Example Usage

```typescript
import { PackageOperationalDefaultsModules } from "@alienplatform/platform-api/models";

let value: PackageOperationalDefaultsModules = {
  heartbeats: "off",
  telemetry: "auto",
  updates: "auto",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `heartbeats`                                                             | [models.PackageModulesHeartbeats](../models/packagemodulesheartbeats.md) | :heavy_check_mark:                                                       | How heartbeat health checks are handled.                                 |
| `telemetry`                                                              | [models.PackageModulesTelemetry](../models/packagemodulestelemetry.md)   | :heavy_check_mark:                                                       | How telemetry (logs, metrics, traces) is handled.                        |
| `updates`                                                                | [models.PackageModulesUpdates](../models/packagemodulesupdates.md)       | :heavy_check_mark:                                                       | How updates are delivered to the deployment.                             |