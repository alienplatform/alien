# DataAwsParameterStore

## Example Usage

```typescript
import { DataAwsParameterStore } from "@alienplatform/platform-api/models/operations";

let value: DataAwsParameterStore = {
  accountId: "<id>",
  events: [],
  parameterMetadataSampled: true,
  prefix: "<value>",
  region: "<value>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "creating",
    partial: false,
    stale: false,
  },
  backend: "awsParameterStore",
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `accountId`                                                                                              | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `events`                                                                                                 | [operations.GetRawResourceHeartbeatEvent31](../../models/operations/getrawresourceheartbeatevent31.md)[] | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `hasMoreParameters`                                                                                      | *boolean*                                                                                                | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `latestModifiedAt`                                                                                       | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)            | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `parameterMetadataSampled`                                                                               | *boolean*                                                                                                | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `prefix`                                                                                                 | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `region`                                                                                                 | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `sampledAdvancedTierCount`                                                                               | *number*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `sampledKmsKeyMetadataPresentCount`                                                                      | *number*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `sampledParameterCount`                                                                                  | *number*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `sampledSecureStringCount`                                                                               | *number*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `sampledStringCount`                                                                                     | *number*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `sampledStringListCount`                                                                                 | *number*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `status`                                                                                                 | [operations.DataStatus31](../../models/operations/datastatus31.md)                                       | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `backend`                                                                                                | *"awsParameterStore"*                                                                                    | :heavy_check_mark:                                                                                       | N/A                                                                                                      |