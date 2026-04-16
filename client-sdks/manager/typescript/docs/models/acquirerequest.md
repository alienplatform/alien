# AcquireRequest

## Example Usage

```typescript
import { AcquireRequest } from "@alienplatform/manager-api/models";

let value: AcquireRequest = {
  session: "<value>",
};
```

## Fields

| Field                                      | Type                                       | Required                                   | Description                                |
| ------------------------------------------ | ------------------------------------------ | ------------------------------------------ | ------------------------------------------ |
| `deploymentIds`                            | *string*[]                                 | :heavy_minus_sign:                         | N/A                                        |
| `limit`                                    | *number*                                   | :heavy_minus_sign:                         | N/A                                        |
| `platforms`                                | [models.Platform](../models/platform.md)[] | :heavy_minus_sign:                         | N/A                                        |
| `session`                                  | *string*                                   | :heavy_check_mark:                         | N/A                                        |
| `statuses`                                 | *string*[]                                 | :heavy_minus_sign:                         | N/A                                        |