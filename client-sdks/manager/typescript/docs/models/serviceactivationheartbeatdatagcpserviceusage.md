# ServiceActivationHeartbeatDataGcpServiceUsage

## Example Usage

```typescript
import { ServiceActivationHeartbeatDataGcpServiceUsage } from "@alienplatform/manager-api/models";

let value: ServiceActivationHeartbeatDataGcpServiceUsage = {
  enabled: false,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  projectId: "<id>",
  serviceName: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "stopping",
    partial: true,
    stale: false,
  },
  backend: "gcpServiceUsage",
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `enabled`                                                                                | *boolean*                                                                                | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `events`                                                                                 | [models.HeartbeatEvent](../models/heartbeatevent.md)[]                                   | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `lastOperationName`                                                                      | *string*                                                                                 | :heavy_minus_sign:                                                                       | N/A                                                                                      |
| `projectId`                                                                              | *string*                                                                                 | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `serviceName`                                                                            | *string*                                                                                 | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `serviceResourceName`                                                                    | *string*                                                                                 | :heavy_minus_sign:                                                                       | N/A                                                                                      |
| `state`                                                                                  | *string*                                                                                 | :heavy_minus_sign:                                                                       | N/A                                                                                      |
| `status`                                                                                 | [models.ServiceActivationHeartbeatStatus](../models/serviceactivationheartbeatstatus.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `title`                                                                                  | *string*                                                                                 | :heavy_minus_sign:                                                                       | N/A                                                                                      |
| `backend`                                                                                | *"gcpServiceUsage"*                                                                      | :heavy_check_mark:                                                                       | N/A                                                                                      |