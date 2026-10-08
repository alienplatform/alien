# TargetDeploymentNetworkByoVpcGcp

## Example Usage

```typescript
import { TargetDeploymentNetworkByoVpcGcp } from "@alienplatform/platform-api/models";

let value: TargetDeploymentNetworkByoVpcGcp = {
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
| `type`                                                                             | [models.TargetDeploymentTypeByoVpcGcp](../models/targetdeploymenttypebyovpcgcp.md) | :heavy_check_mark:                                                                 | N/A                                                                                |