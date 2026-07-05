# DataGcpFirestore

## Example Usage

```typescript
import { DataGcpFirestore } from "@alienplatform/platform-api/models/operations";

let value: DataGcpFirestore = {
  cmekEnabled: false,
  databaseName: "<value>",
  sourceInfoPresent: false,
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "running",
    partial: false,
    stale: false,
  },
  backend: "gcpFirestore",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `appEngineIntegrationMode`                                         | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `cmekEnabled`                                                      | *boolean*                                                          | :heavy_check_mark:                                                 | N/A                                                                |
| `concurrencyMode`                                                  | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `createTime`                                                       | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `databaseEdition`                                                  | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `databaseName`                                                     | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `databaseType`                                                     | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `deleteProtectionState`                                            | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `deleteTime`                                                       | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `earliestVersionTime`                                              | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `endpoint`                                                         | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `locationId`                                                       | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `pointInTimeRecoveryEnablement`                                    | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `projectId`                                                        | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `sourceInfoPresent`                                                | *boolean*                                                          | :heavy_check_mark:                                                 | N/A                                                                |
| `status`                                                           | [operations.DataStatus30](../../models/operations/datastatus30.md) | :heavy_check_mark:                                                 | N/A                                                                |
| `updateTime`                                                       | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `versionRetentionPeriod`                                           | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `backend`                                                          | *"gcpFirestore"*                                                   | :heavy_check_mark:                                                 | N/A                                                                |