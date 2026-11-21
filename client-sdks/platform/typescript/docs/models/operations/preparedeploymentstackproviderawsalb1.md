# PrepareDeploymentStackProviderAwsAlb1

## Example Usage

```typescript
import { PrepareDeploymentStackProviderAwsAlb1 } from "@alienplatform/platform-api/models/operations";

let value: PrepareDeploymentStackProviderAwsAlb1 = {
  provider: "awsAlb",
  scheme: "<value>",
  targetType: "<value>",
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `ipAddressType`                                                                                                              | *string*                                                                                                                     | :heavy_minus_sign:                                                                                                           | Optional ALB IP address type, such as `dualstack`.                                                                           |
| `provider`                                                                                                                   | [operations.PrepareDeploymentStackProviderAwsAlbEnum1](../../models/operations/preparedeploymentstackproviderawsalbenum1.md) | :heavy_check_mark:                                                                                                           | N/A                                                                                                                          |
| `scheme`                                                                                                                     | *string*                                                                                                                     | :heavy_check_mark:                                                                                                           | Internet-facing or internal ALB scheme.                                                                                      |
| `subnetIds`                                                                                                                  | *string*[]                                                                                                                   | :heavy_minus_sign:                                                                                                           | Explicit subnet IDs when the profile cannot rely on controller discovery.                                                    |
| `targetType`                                                                                                                 | *string*                                                                                                                     | :heavy_check_mark:                                                                                                           | ALB target type, usually `ip`.                                                                                               |