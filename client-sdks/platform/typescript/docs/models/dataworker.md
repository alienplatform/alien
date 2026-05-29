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
        observedAt: new Date("2024-05-06T03:27:45.769Z"),
        severity: "info",
      },
    ],
    imagePathPresent: true,
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "api-unavailable",
          severity: "error",
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