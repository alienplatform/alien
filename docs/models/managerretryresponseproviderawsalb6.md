# ManagerRetryResponseProviderAwsAlb6

## Example Usage

```typescript
import { ManagerRetryResponseProviderAwsAlb6 } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponseProviderAwsAlb6 = {
  provider: "awsAlb",
  scheme: "<value>",
  targetType: "<value>",
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `ipAddressType`                                                                                        | *string*                                                                                               | :heavy_minus_sign:                                                                                     | Optional ALB IP address type, such as `dualstack`.                                                     |
| `provider`                                                                                             | [models.ManagerRetryResponseProviderAwsAlbEnum6](../models/managerretryresponseproviderawsalbenum6.md) | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `scheme`                                                                                               | *string*                                                                                               | :heavy_check_mark:                                                                                     | Internet-facing or internal ALB scheme.                                                                |
| `subnetIds`                                                                                            | *string*[]                                                                                             | :heavy_minus_sign:                                                                                     | Explicit subnet IDs when the profile cannot rely on controller discovery.                              |
| `targetType`                                                                                           | *string*                                                                                               | :heavy_check_mark:                                                                                     | ALB target type, usually `ip`.                                                                         |