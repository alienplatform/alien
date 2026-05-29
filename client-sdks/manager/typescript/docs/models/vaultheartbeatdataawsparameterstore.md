# VaultHeartbeatDataAwsParameterStore

## Example Usage

```typescript
import { VaultHeartbeatDataAwsParameterStore } from "@alienplatform/manager-api/models";

let value: VaultHeartbeatDataAwsParameterStore = {
  accountId: "<id>",
  events: [],
  parameterMetadataSampled: false,
  prefix: "<value>",
  region: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "updating",
    partial: true,
    stale: true,
  },
  backend: "awsParameterStore",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `accountId`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `events`                                                                                      | [models.HeartbeatEvent](../models/heartbeatevent.md)[]                                        | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `hasMoreParameters`                                                                           | *boolean*                                                                                     | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `latestModifiedAt`                                                                            | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `parameterMetadataSampled`                                                                    | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `prefix`                                                                                      | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `region`                                                                                      | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `sampledAdvancedTierCount`                                                                    | *number*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `sampledKmsKeyMetadataPresentCount`                                                           | *number*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `sampledParameterCount`                                                                       | *number*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `sampledSecureStringCount`                                                                    | *number*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `sampledStringCount`                                                                          | *number*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `sampledStringListCount`                                                                      | *number*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `status`                                                                                      | [models.VaultHeartbeatStatus](../models/vaultheartbeatstatus.md)                              | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `backend`                                                                                     | *"awsParameterStore"*                                                                         | :heavy_check_mark:                                                                            | N/A                                                                                           |