# AwsKmsKeyHeartbeatData

## Example Usage

```typescript
import { AwsKmsKeyHeartbeatData } from "@alienplatform/manager-api/models";

let value: AwsKmsKeyHeartbeatData = {
  enabled: true,
  keyArn: "<value>",
  keySpec: "<value>",
  keyState: "<value>",
  keyUsage: "<value>",
  status: {
    health: "healthy",
    lifecycle: "running",
  },
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `enabled`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `keyArn`                                                     | *string*                                                     | :heavy_check_mark:                                           | N/A                                                          |
| `keySpec`                                                    | *string*                                                     | :heavy_check_mark:                                           | N/A                                                          |
| `keyState`                                                   | *string*                                                     | :heavy_check_mark:                                           | N/A                                                          |
| `keyUsage`                                                   | *string*                                                     | :heavy_check_mark:                                           | N/A                                                          |
| `status`                                                     | [models.KeyHeartbeatStatus](../models/keyheartbeatstatus.md) | :heavy_check_mark:                                           | N/A                                                          |