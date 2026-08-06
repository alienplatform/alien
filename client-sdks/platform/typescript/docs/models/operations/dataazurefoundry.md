# DataAzureFoundry

## Example Usage

```typescript
import { DataAzureFoundry } from "@alienplatform/platform-api/models/operations";

let value: DataAzureFoundry = {
  accountName: "<value>",
  availability: {
    catalogRevision: "<value>",
    models: [
      {
        accessTest: "verified",
        availability: "available",
        blockers: [],
        clientApis: [
          "anthropic-messages",
        ],
        publicModelId: "<id>",
      },
    ],
    source: "azure-foundry",
  },
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "stopping",
    partial: false,
    stale: false,
  },
  backend: "azureFoundry",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `accountName`                                                        | *string*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `availability`                                                       | [operations.Availability3](../../models/operations/availability3.md) | :heavy_check_mark:                                                   | N/A                                                                  |
| `endpoint`                                                           | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `location`                                                           | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `resourceGroup`                                                      | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `status`                                                             | [operations.DataStatus68](../../models/operations/datastatus68.md)   | :heavy_check_mark:                                                   | N/A                                                                  |
| `backend`                                                            | *"azureFoundry"*                                                     | :heavy_check_mark:                                                   | N/A                                                                  |