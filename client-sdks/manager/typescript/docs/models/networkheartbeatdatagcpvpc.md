# NetworkHeartbeatDataGcpVpc

## Example Usage

```typescript
import { NetworkHeartbeatDataGcpVpc } from "@alienplatform/manager-api/models";

let value: NetworkHeartbeatDataGcpVpc = {
  isByoVpc: false,
  status: {
    collectionIssues: [],
    health: "healthy",
    lifecycle: "stopped",
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