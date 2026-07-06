# DataGcpVpc

## Example Usage

```typescript
import { DataGcpVpc } from "@alienplatform/platform-api/models";

let value: DataGcpVpc = {
  isByoVpc: true,
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "deleting",
    partial: true,
    stale: false,
  },
  backend: "gcpVpc",
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `cidrBlock`                                                | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `cloudNatName`                                             | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `firewallName`                                             | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `isByoVpc`                                                 | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |
| `networkName`                                              | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `networkSelfLink`                                          | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `region`                                                   | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `routerName`                                               | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `status`                                                   | [models.HeartbeatStatus45](../models/heartbeatstatus45.md) | :heavy_check_mark:                                         | N/A                                                        |
| `subnetworkName`                                           | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `subnetworkSelfLink`                                       | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `backend`                                                  | *"gcpVpc"*                                                 | :heavy_check_mark:                                         | N/A                                                        |