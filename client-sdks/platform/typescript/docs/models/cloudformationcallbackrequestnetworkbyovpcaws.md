# CloudFormationCallbackRequestNetworkByoVpcAws

## Example Usage

```typescript
import { CloudFormationCallbackRequestNetworkByoVpcAws } from "@alienplatform/platform-api/models";

let value: CloudFormationCallbackRequestNetworkByoVpcAws = {
  privateSubnetIds: [
    "<value 1>",
    "<value 2>",
  ],
  publicSubnetIds: [
    "<value 1>",
    "<value 2>",
  ],
  type: "byo-vpc-aws",
  vpcId: "<id>",
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `privateSubnetIds`                                                                                           | *string*[]                                                                                                   | :heavy_check_mark:                                                                                           | IDs of private subnets                                                                                       |
| `publicSubnetIds`                                                                                            | *string*[]                                                                                                   | :heavy_check_mark:                                                                                           | IDs of public subnets (required for public ingress)                                                          |
| `securityGroupIds`                                                                                           | *string*[]                                                                                                   | :heavy_minus_sign:                                                                                           | Optional security group IDs to use                                                                           |
| `type`                                                                                                       | [models.CloudFormationCallbackRequestTypeByoVpcAws](../models/cloudformationcallbackrequesttypebyovpcaws.md) | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `vpcId`                                                                                                      | *string*                                                                                                     | :heavy_check_mark:                                                                                           | The ID of the existing VPC                                                                                   |