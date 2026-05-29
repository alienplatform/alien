# Data4

## Example Usage

```typescript
import { Data4 } from "@alienplatform/platform-api/models/operations";

let value: Data4 = {
  events: [],
  name: "<value>",
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "updating",
    partial: true,
    stale: true,
  },
  workloadProfileCount: 739106,
  workloadProfiles: [
    {
      name: "<value>",
      workloadProfileType: "<value>",
    },
  ],
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `customDomainVerificationId`                                                                             | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `defaultDomain`                                                                                          | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `eventStreamEndpoint`                                                                                    | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `events`                                                                                                 | [operations.GetRawResourceHeartbeatEvent58](../../models/operations/getrawresourceheartbeatevent58.md)[] | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `infrastructureResourceGroup`                                                                            | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `kind`                                                                                                   | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `location`                                                                                               | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `name`                                                                                                   | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `provisioningState`                                                                                      | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `resourceGroup`                                                                                          | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `resourceId`                                                                                             | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `staticIp`                                                                                               | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `status`                                                                                                 | [operations.DataStatus58](../../models/operations/datastatus58.md)                                       | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `workloadProfileCount`                                                                                   | *number*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `workloadProfiles`                                                                                       | [operations.WorkloadProfile](../../models/operations/workloadprofile.md)[]                               | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `zoneRedundant`                                                                                          | *boolean*                                                                                                | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |