# HeartbeatAvailable

## Example Usage

```typescript
import { HeartbeatAvailable } from "@alienplatform/platform-api/models/operations";

let value: HeartbeatAvailable = {
  status: "available",
  deploymentId: "<id>",
  resourceId: "<id>",
  resourceType: "<value>",
  backend: "<value>",
  controllerPlatform: "<value>",
  observedAt: new Date("2024-06-21T00:18:54.209Z"),
  staleAt: new Date("2024-03-07T10:23:33.467Z"),
  platformStale: false,
  heartbeat: {
    backend: "azure",
    controllerPlatform: "gcp",
    data: {
      data: {
        name: "<value>",
        privateEndpointConnectionCount: 152029,
        status: {
          collectionIssues: [
            {
              message: "<value>",
              reason: "collection-failed",
              severity: "warning",
              source: "<value>",
            },
          ],
          health: "healthy",
          lifecycle: "running",
          partial: true,
          stale: true,
        },
      },
      resourceType: "azure_service_bus_namespace",
    },
    observedAt: new Date("2024-03-08T08:46:36.237Z"),
    raw: [],
    resourceId: "<id>",
    resourceType: "<value>",
  },
  raw: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `status`                                                                                      | *"available"*                                                                                 | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `deploymentId`                                                                                | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `resourceId`                                                                                  | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `resourceType`                                                                                | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `backend`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `controllerPlatform`                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `observedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `staleAt`                                                                                     | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `platformStale`                                                                               | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `heartbeat`                                                                                   | [operations.Heartbeat](../../models/operations/heartbeat.md)                                  | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `raw`                                                                                         | *any*[]                                                                                       | :heavy_check_mark:                                                                            | N/A                                                                                           |