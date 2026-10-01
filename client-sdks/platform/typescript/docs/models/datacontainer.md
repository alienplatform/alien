# DataContainer

## Example Usage

```typescript
import { DataContainer } from "@alienplatform/platform-api/models";

let value: DataContainer = {
  data: {
    attentionCount: 486054,
    containerId: "<id>",
    events: [],
    replicaUnits: [],
    replicas: {},
    schedulingMode: "stateful",
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "not-installed",
          severity: "info",
          source: "<value>",
        },
      ],
      health: "degraded",
      lifecycle: "failed",
      partial: false,
      stale: true,
    },
    backend: "horizonPlatform",
  },
  resourceType: "container",
};
```

## Fields

| Field                                   | Type                                    | Required                                | Description                             |
| --------------------------------------- | --------------------------------------- | --------------------------------------- | --------------------------------------- |
| `data`                                  | *models.SyncReconcileRequestDataUnion3* | :heavy_check_mark:                      | N/A                                     |
| `resourceType`                          | *"container"*                           | :heavy_check_mark:                      | N/A                                     |