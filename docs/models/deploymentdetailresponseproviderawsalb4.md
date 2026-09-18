# DeploymentDetailResponseProviderAwsAlb4

## Example Usage

```typescript
import { DeploymentDetailResponseProviderAwsAlb4 } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseProviderAwsAlb4 = {
  provider: "awsAlb",
  scheme: "<value>",
  targetType: "<value>",
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `ipAddressType`                                                                                                | *string*                                                                                                       | :heavy_minus_sign:                                                                                             | Optional ALB IP address type, such as `dualstack`.                                                             |
| `provider`                                                                                                     | [models.DeploymentDetailResponseProviderAwsAlbEnum4](../models/deploymentdetailresponseproviderawsalbenum4.md) | :heavy_check_mark:                                                                                             | N/A                                                                                                            |
| `scheme`                                                                                                       | *string*                                                                                                       | :heavy_check_mark:                                                                                             | Internet-facing or internal ALB scheme.                                                                        |
| `subnetIds`                                                                                                    | *string*[]                                                                                                     | :heavy_minus_sign:                                                                                             | Explicit subnet IDs when the profile cannot rely on controller discovery.                                      |
| `targetType`                                                                                                   | *string*                                                                                                       | :heavy_check_mark:                                                                                             | ALB target type, usually `ip`.                                                                                 |