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
        data: {
          enabled: true,
          keyArn: "<value>",
          keySpec: "<value>",
          keyState: "<value>",
          keyUsage: "<value>",
          status: {
            health: "healthy",
            lifecycle: "failed",
          },
        },
        provider: "aws-kms",
      },
      resourceType: "key",
    },
    observedAt: new Date("2024-04-26T23:19:48.455Z"),
    raw: [
      {
        body: "<value>",
        collectedAt: new Date("2025-01-29T23:35:36.058Z"),
        format: "json",
        source: "<value>",
        truncated: true,
      },
    ],
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