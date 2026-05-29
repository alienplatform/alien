# NetworkHeartbeatDataGcpVpc

## Example Usage

```typescript
import { NetworkHeartbeatDataGcpVpc } from "@alienplatform/manager-api/models";

let value: NetworkHeartbeatDataGcpVpc = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  isByoVpc: true,
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "scaling",
    partial: true,
    stale: true,
  },
  backend: "gcpVpc",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `cidrBlock`                                                          | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `cloudNatName`                                                       | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `events`                                                             | [models.HeartbeatEvent](../models/heartbeatevent.md)[]               | :heavy_check_mark:                                                   | N/A                                                                  |
| `firewallName`                                                       | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `isByoVpc`                                                           | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |
| `networkName`                                                        | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `networkSelfLink`                                                    | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `region`                                                             | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `routerName`                                                         | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `status`                                                             | [models.NetworkHeartbeatStatus](../models/networkheartbeatstatus.md) | :heavy_check_mark:                                                   | N/A                                                                  |
| `subnetworkName`                                                     | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `subnetworkSelfLink`                                                 | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `backend`                                                            | *"gcpVpc"*                                                           | :heavy_check_mark:                                                   | N/A                                                                  |