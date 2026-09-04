# GcpCloudKmsKeyHeartbeatData

## Example Usage

```typescript
import { GcpCloudKmsKeyHeartbeatData } from "@alienplatform/manager-api/models";

let value: GcpCloudKmsKeyHeartbeatData = {
  cryptoKeyName: "<value>",
  purpose: "<value>",
  status: {
    health: "healthy",
    lifecycle: "running",
  },
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `algorithm`                                                  | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `cryptoKeyName`                                              | *string*                                                     | :heavy_check_mark:                                           | N/A                                                          |
| `primaryState`                                               | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `primaryVersion`                                             | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `purpose`                                                    | *string*                                                     | :heavy_check_mark:                                           | N/A                                                          |
| `status`                                                     | [models.KeyHeartbeatStatus](../models/keyheartbeatstatus.md) | :heavy_check_mark:                                           | N/A                                                          |