# AzureContainerAppsEnvironmentHeartbeatData

## Example Usage

```typescript
import { AzureContainerAppsEnvironmentHeartbeatData } from "@alienplatform/manager-api/models";

let value: AzureContainerAppsEnvironmentHeartbeatData = {
  events: [],
  name: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "stopping",
    partial: false,
    stale: false,
  },
  workloadProfileCount: 797933,
  workloadProfiles: [],
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `customDomainVerificationId`                                                                                       | *string*                                                                                                           | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |
| `defaultDomain`                                                                                                    | *string*                                                                                                           | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |
| `eventStreamEndpoint`                                                                                              | *string*                                                                                                           | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |
| `events`                                                                                                           | [models.HeartbeatEvent](../models/heartbeatevent.md)[]                                                             | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |
| `infrastructureResourceGroup`                                                                                      | *string*                                                                                                           | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |
| `kind`                                                                                                             | *string*                                                                                                           | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |
| `location`                                                                                                         | *string*                                                                                                           | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |
| `name`                                                                                                             | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |
| `provisioningState`                                                                                                | *string*                                                                                                           | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |
| `resourceGroup`                                                                                                    | *string*                                                                                                           | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |
| `resourceId`                                                                                                       | *string*                                                                                                           | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |
| `staticIp`                                                                                                         | *string*                                                                                                           | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |
| `status`                                                                                                           | [models.AzureContainerAppsEnvironmentHeartbeatStatus](../models/azurecontainerappsenvironmentheartbeatstatus.md)   | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |
| `workloadProfileCount`                                                                                             | *number*                                                                                                           | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |
| `workloadProfiles`                                                                                                 | [models.AzureContainerAppsEnvironmentWorkloadProfile](../models/azurecontainerappsenvironmentworkloadprofile.md)[] | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |
| `zoneRedundant`                                                                                                    | *boolean*                                                                                                          | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |