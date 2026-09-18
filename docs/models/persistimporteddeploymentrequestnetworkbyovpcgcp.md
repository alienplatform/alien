# PersistImportedDeploymentRequestNetworkByoVpcGcp

## Example Usage

```typescript
import { PersistImportedDeploymentRequestNetworkByoVpcGcp } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestNetworkByoVpcGcp = {
  networkName: "<value>",
  region: "<value>",
  subnetName: "<value>",
  type: "byo-vpc-gcp",
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `networkName`                                                                                                      | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | The name of the existing VPC network                                                                               |
| `region`                                                                                                           | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | The region of the subnet                                                                                           |
| `subnetName`                                                                                                       | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | The name of the subnet to use                                                                                      |
| `type`                                                                                                             | [models.PersistImportedDeploymentRequestTypeByoVpcGcp](../models/persistimporteddeploymentrequesttypebyovpcgcp.md) | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |