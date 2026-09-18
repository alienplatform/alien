# DataAwsKms

## Example Usage

```typescript
import { DataAwsKms } from "@alienplatform/platform-api/models";

let value: DataAwsKms = {
  data: {
    enabled: true,
    keyArn: "<value>",
    keySpec: "<value>",
    keyState: "<value>",
    keyUsage: "<value>",
    status: {
      health: "unhealthy",
      lifecycle: "stopped",
    },
  },
  provider: "aws-kms",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `data`                                                                     | [models.SyncReconcileRequestData6](../models/syncreconcilerequestdata6.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `provider`                                                                 | *"aws-kms"*                                                                | :heavy_check_mark:                                                         | N/A                                                                        |