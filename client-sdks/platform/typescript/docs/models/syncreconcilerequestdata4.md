# SyncReconcileRequestData4

## Example Usage

```typescript
import { SyncReconcileRequestData4 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestData4 = {
  name: "<value>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "scaling",
    partial: true,
    stale: true,
  },
  workloadProfileCount: 977521,
  workloadProfiles: [
    {
      name: "<value>",
      workloadProfileType: "<value>",
    },
  ],
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `customDomainVerificationId`                               | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `defaultDomain`                                            | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `eventStreamEndpoint`                                      | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `infrastructureResourceGroup`                              | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `kind`                                                     | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `location`                                                 | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `name`                                                     | *string*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `provisioningState`                                        | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `resourceGroup`                                            | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `resourceId`                                               | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `staticIp`                                                 | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `status`                                                   | [models.HeartbeatStatus62](../models/heartbeatstatus62.md) | :heavy_check_mark:                                         | N/A                                                        |
| `workloadProfileCount`                                     | *number*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `workloadProfiles`                                         | [models.WorkloadProfile](../models/workloadprofile.md)[]   | :heavy_check_mark:                                         | N/A                                                        |
| `zoneRedundant`                                            | *boolean*                                                  | :heavy_minus_sign:                                         | N/A                                                        |