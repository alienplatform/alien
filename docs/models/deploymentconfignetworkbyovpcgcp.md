# DeploymentConfigNetworkByoVpcGcp

## Example Usage

```typescript
import { DeploymentConfigNetworkByoVpcGcp } from "@alienplatform/platform-api/models";

let value: DeploymentConfigNetworkByoVpcGcp = {
  networkName: "<value>",
  region: "<value>",
  subnetName: "<value>",
  type: "byo-vpc-gcp",
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `networkName`                                                                      | *string*                                                                           | :heavy_check_mark:                                                                 | The name of the existing VPC network                                               |
| `region`                                                                           | *string*                                                                           | :heavy_check_mark:                                                                 | The region of the subnet                                                           |
| `subnetName`                                                                       | *string*                                                                           | :heavy_check_mark:                                                                 | The name of the subnet to use                                                      |
| `type`                                                                             | [models.DeploymentConfigTypeByoVpcGcp](../models/deploymentconfigtypebyovpcgcp.md) | :heavy_check_mark:                                                                 | N/A                                                                                |