# SyncReconcileRequestData9

## Example Usage

```typescript
import { SyncReconcileRequestData9 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestData9 = {
  data: {
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "api-unavailable",
          severity: "error",
          source: "<value>",
        },
      ],
      health: "unhealthy",
      lifecycle: "stopping",
      partial: true,
      stale: true,
    },
  },
  resourceType: "<value>",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `data`                                                                           | [models.SyncReconcileRequestDataData](../models/syncreconcilerequestdatadata.md) | :heavy_check_mark:                                                               | N/A                                                                              |
| `resourceType`                                                                   | *string*                                                                         | :heavy_check_mark:                                                               | The type of a resource an extension defines, such as `acme/dns-zone`.            |