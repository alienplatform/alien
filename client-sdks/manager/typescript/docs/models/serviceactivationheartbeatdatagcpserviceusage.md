# ServiceActivationHeartbeatDataGcpServiceUsage

## Example Usage

```typescript
import { ServiceActivationHeartbeatDataGcpServiceUsage } from "@alienplatform/manager-api/models";

let value: ServiceActivationHeartbeatDataGcpServiceUsage = {
  enabled: false,
  projectId: "<id>",
  serviceName: "<value>",
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "unknown",
    partial: false,
    stale: true,
  },
  backend: "gcpServiceUsage",
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `enabled`                                                                                | *boolean*                                                                                | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `lastOperationName`                                                                      | *string*                                                                                 | :heavy_minus_sign:                                                                       | N/A                                                                                      |
| `projectId`                                                                              | *string*                                                                                 | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `serviceName`                                                                            | *string*                                                                                 | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `serviceResourceName`                                                                    | *string*                                                                                 | :heavy_minus_sign:                                                                       | N/A                                                                                      |
| `state`                                                                                  | *string*                                                                                 | :heavy_minus_sign:                                                                       | N/A                                                                                      |
| `status`                                                                                 | [models.ServiceActivationHeartbeatStatus](../models/serviceactivationheartbeatstatus.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `title`                                                                                  | *string*                                                                                 | :heavy_minus_sign:                                                                       | N/A                                                                                      |
| `backend`                                                                                | *"gcpServiceUsage"*                                                                      | :heavy_check_mark:                                                                       | N/A                                                                                      |