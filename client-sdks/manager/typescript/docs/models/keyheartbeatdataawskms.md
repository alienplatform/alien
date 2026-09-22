# KeyHeartbeatDataAwsKms

## Example Usage

```typescript
import { KeyHeartbeatDataAwsKms } from "@alienplatform/manager-api/models";

let value: KeyHeartbeatDataAwsKms = {
  data: {
    enabled: false,
    keyArn: "<value>",
    keySpec: "<value>",
    keyState: "<value>",
    keyUsage: "<value>",
    status: {
      health: "unknown",
      lifecycle: "deleted",
    },
  },
  provider: "aws-kms",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `data`                                                               | [models.AwsKmsKeyHeartbeatData](../models/awskmskeyheartbeatdata.md) | :heavy_check_mark:                                                   | N/A                                                                  |
| `provider`                                                           | *"aws-kms"*                                                          | :heavy_check_mark:                                                   | N/A                                                                  |