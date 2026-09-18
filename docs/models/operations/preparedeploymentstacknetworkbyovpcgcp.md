# PrepareDeploymentStackNetworkByoVpcGcp

## Example Usage

```typescript
import { PrepareDeploymentStackNetworkByoVpcGcp } from "@alienplatform/platform-api/models/operations";

let value: PrepareDeploymentStackNetworkByoVpcGcp = {
  networkName: "<value>",
  region: "<value>",
  subnetName: "<value>",
  type: "byo-vpc-gcp",
};
```

## Fields

| Field                                                                                                            | Type                                                                                                             | Required                                                                                                         | Description                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `networkName`                                                                                                    | *string*                                                                                                         | :heavy_check_mark:                                                                                               | The name of the existing VPC network                                                                             |
| `region`                                                                                                         | *string*                                                                                                         | :heavy_check_mark:                                                                                               | The region of the subnet                                                                                         |
| `subnetName`                                                                                                     | *string*                                                                                                         | :heavy_check_mark:                                                                                               | The name of the subnet to use                                                                                    |
| `type`                                                                                                           | [operations.PrepareDeploymentStackTypeByoVpcGcp](../../models/operations/preparedeploymentstacktypebyovpcgcp.md) | :heavy_check_mark:                                                                                               | N/A                                                                                                              |