# HistoryUnavailable

## Example Usage

```typescript
import { HistoryUnavailable } from "@alienplatform/platform-api/models";

let value: HistoryUnavailable = {
  status: "unavailable",
  reason: "not-configured",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `status`                                                           | *"unavailable"*                                                    | :heavy_check_mark:                                                 | N/A                                                                |
| `reason`                                                           | [models.DeploymentPulseReason](../models/deploymentpulsereason.md) | :heavy_check_mark:                                                 | N/A                                                                |