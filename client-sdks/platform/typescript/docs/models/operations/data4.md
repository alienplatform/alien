# Data4

## Example Usage

```typescript
import { Data4 } from "@alienplatform/platform-api/models/operations";

let value: Data4 = {
  name: "<value>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "stopped",
    partial: true,
    stale: true,
  },
  workloadProfileCount: 17115,
  workloadProfiles: [
    {
      name: "<value>",
      workloadProfileType: "<value>",
    },
  ],
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `customDomainVerificationId`                                               | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `defaultDomain`                                                            | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `eventStreamEndpoint`                                                      | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `infrastructureResourceGroup`                                              | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `kind`                                                                     | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `location`                                                                 | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `name`                                                                     | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `provisioningState`                                                        | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `resourceGroup`                                                            | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `resourceId`                                                               | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `staticIp`                                                                 | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `status`                                                                   | [operations.DataStatus58](../../models/operations/datastatus58.md)         | :heavy_check_mark:                                                         | N/A                                                                        |
| `workloadProfileCount`                                                     | *number*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `workloadProfiles`                                                         | [operations.WorkloadProfile](../../models/operations/workloadprofile.md)[] | :heavy_check_mark:                                                         | N/A                                                                        |
| `zoneRedundant`                                                            | *boolean*                                                                  | :heavy_minus_sign:                                                         | N/A                                                                        |