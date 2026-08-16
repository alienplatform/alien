# ResourceHeartbeatStatus71

## Example Usage

```typescript
import { ResourceHeartbeatStatus71 } from "@alienplatform/platform-api/models";

let value: ResourceHeartbeatStatus71 = {
  health: "degraded",
  lifecycle: "updating",
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `health`                                                   | [models.DataHealth71](../models/datahealth71.md)           | :heavy_check_mark:                                         | N/A                                                        |
| `lifecycle`                                                | [models.StatusLifecycle71](../models/statuslifecycle71.md) | :heavy_check_mark:                                         | N/A                                                        |
| `message`                                                  | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |