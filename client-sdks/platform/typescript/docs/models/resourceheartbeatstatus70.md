# ResourceHeartbeatStatus70

## Example Usage

```typescript
import { ResourceHeartbeatStatus70 } from "@alienplatform/platform-api/models";

let value: ResourceHeartbeatStatus70 = {
  health: "unhealthy",
  lifecycle: "stopping",
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `health`                                                   | [models.DataHealth70](../models/datahealth70.md)           | :heavy_check_mark:                                         | N/A                                                        |
| `lifecycle`                                                | [models.StatusLifecycle70](../models/statuslifecycle70.md) | :heavy_check_mark:                                         | N/A                                                        |
| `message`                                                  | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |