# DataAzureFoundry

## Example Usage

```typescript
import { DataAzureFoundry } from "@alienplatform/platform-api/models";

let value: DataAzureFoundry = {
  accountName: "<value>",
  availability: {
    catalogRevision: "<value>",
    models: [
      {
        accessTest: "verified",
        availability: "available",
        blockers: [
          "quota-configuration-required",
        ],
        clientApis: [
          "open-ai-responses",
        ],
        publicModelId: "<id>",
      },
    ],
    source: "aws-bedrock",
  },
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "stopped",
    partial: false,
    stale: false,
  },
  backend: "azureFoundry",
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `accountName`                                                                              | *string*                                                                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `availability`                                                                             | [models.SyncReconcileRequestAvailability3](../models/syncreconcilerequestavailability3.md) | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `endpoint`                                                                                 | *string*                                                                                   | :heavy_minus_sign:                                                                         | N/A                                                                                        |
| `location`                                                                                 | *string*                                                                                   | :heavy_minus_sign:                                                                         | N/A                                                                                        |
| `resourceGroup`                                                                            | *string*                                                                                   | :heavy_minus_sign:                                                                         | N/A                                                                                        |
| `status`                                                                                   | [models.ResourceHeartbeatStatus68](../models/resourceheartbeatstatus68.md)                 | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `backend`                                                                                  | *"azureFoundry"*                                                                           | :heavy_check_mark:                                                                         | N/A                                                                                        |