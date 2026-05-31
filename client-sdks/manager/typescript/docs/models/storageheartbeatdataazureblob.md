# StorageHeartbeatDataAzureBlob

## Example Usage

```typescript
import { StorageHeartbeatDataAzureBlob } from "@alienplatform/manager-api/models";

let value: StorageHeartbeatDataAzureBlob = {
  name: "<value>",
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "stopping",
    partial: true,
    stale: true,
  },
  backend: "azureBlob",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `accessTier`                                                         | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `accountKind`                                                        | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `allowBlobPublicAccess`                                              | *boolean*                                                            | :heavy_minus_sign:                                                   | N/A                                                                  |
| `blobDeleteRetentionDays`                                            | *number*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `blobDeleteRetentionEnabled`                                         | *boolean*                                                            | :heavy_minus_sign:                                                   | N/A                                                                  |
| `blobEncryptionEnabled`                                              | *boolean*                                                            | :heavy_minus_sign:                                                   | N/A                                                                  |
| `blobVersioningEnabled`                                              | *boolean*                                                            | :heavy_minus_sign:                                                   | N/A                                                                  |
| `changeFeedEnabled`                                                  | *boolean*                                                            | :heavy_minus_sign:                                                   | N/A                                                                  |
| `changeFeedRetentionDays`                                            | *number*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `containerDeleteRetentionDays`                                       | *number*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `containerDeleteRetentionEnabled`                                    | *boolean*                                                            | :heavy_minus_sign:                                                   | N/A                                                                  |
| `containerPublicAccess`                                              | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `encryptionKeySource`                                                | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `fileEncryptionEnabled`                                              | *boolean*                                                            | :heavy_minus_sign:                                                   | N/A                                                                  |
| `location`                                                           | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `name`                                                               | *string*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `primaryLocation`                                                    | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `provisioningState`                                                  | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `publicNetworkAccess`                                                | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `queueEncryptionEnabled`                                             | *boolean*                                                            | :heavy_minus_sign:                                                   | N/A                                                                  |
| `resourceGroup`                                                      | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `secondaryLocation`                                                  | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `skuName`                                                            | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `skuTier`                                                            | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `status`                                                             | [models.StorageHeartbeatStatus](../models/storageheartbeatstatus.md) | :heavy_check_mark:                                                   | N/A                                                                  |
| `statusOfPrimary`                                                    | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `statusOfSecondary`                                                  | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `storageAccountName`                                                 | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `tableEncryptionEnabled`                                             | *boolean*                                                            | :heavy_minus_sign:                                                   | N/A                                                                  |
| `backend`                                                            | *"azureBlob"*                                                        | :heavy_check_mark:                                                   | N/A                                                                  |