# DataGcpFirestore

## Example Usage

```typescript
import { DataGcpFirestore } from "@alienplatform/platform-api/models/operations";

let value: DataGcpFirestore = {
  cmekEnabled: false,
  databaseName: "<value>",
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-13T08:16:25.783Z"),
      severity: "info",
    },
  ],
  sourceInfoPresent: true,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "creating",
    partial: false,
    stale: true,
  },
  backend: "gcpFirestore",
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `appEngineIntegrationMode`                                                                               | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `cmekEnabled`                                                                                            | *boolean*                                                                                                | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `concurrencyMode`                                                                                        | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `createTime`                                                                                             | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `databaseEdition`                                                                                        | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `databaseName`                                                                                           | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `databaseType`                                                                                           | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `deleteProtectionState`                                                                                  | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `deleteTime`                                                                                             | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `earliestVersionTime`                                                                                    | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `endpoint`                                                                                               | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `events`                                                                                                 | [operations.GetRawResourceHeartbeatEvent28](../../models/operations/getrawresourceheartbeatevent28.md)[] | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `locationId`                                                                                             | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `pointInTimeRecoveryEnablement`                                                                          | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `projectId`                                                                                              | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `sourceInfoPresent`                                                                                      | *boolean*                                                                                                | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `status`                                                                                                 | [operations.DataStatus28](../../models/operations/datastatus28.md)                                       | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `updateTime`                                                                                             | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `versionRetentionPeriod`                                                                                 | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `backend`                                                                                                | *"gcpFirestore"*                                                                                         | :heavy_check_mark:                                                                                       | N/A                                                                                                      |