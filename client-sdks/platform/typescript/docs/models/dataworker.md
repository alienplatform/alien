# DataWorker

## Example Usage

```typescript
import { DataWorker } from "@alienplatform/platform-api/models";

let value: DataWorker = {
  data: {
    commandSupported: false,
    events: [
      {
        kind: "<value>",
        message: "<value>",
        severity: "info",
        timestamp: new Date("2024-07-21T11:12:11.792Z"),
      },
    ],
    imagePathPresent: true,
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "not-installed",
          severity: "warning",
          source: "<value>",
        },
      ],
      health: "degraded",
      lifecycle: "updating",
      partial: true,
      stale: false,
    },
    triggerCount: 71218,
    backend: "local",
  },
  resourceType: "worker",
};
```

## Fields

| Field                                   | Type                                    | Required                                | Description                             |
| --------------------------------------- | --------------------------------------- | --------------------------------------- | --------------------------------------- |
| `data`                                  | *models.SyncReconcileRequestDataUnion2* | :heavy_check_mark:                      | N/A                                     |
| `resourceType`                          | *"worker"*                              | :heavy_check_mark:                      | N/A                                     |