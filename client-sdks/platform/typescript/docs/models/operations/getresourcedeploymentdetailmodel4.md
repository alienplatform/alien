# GetResourceDeploymentDetailModel4

## Example Usage

```typescript
import { GetResourceDeploymentDetailModel4 } from "@alienplatform/platform-api/models/operations";

let value: GetResourceDeploymentDetailModel4 = {
  accessTest: "verified",
  availability: "available",
  blockers: [
    "quota-configuration-required",
  ],
  clientApis: [
    "anthropic-messages",
  ],
  publicModelId: "<id>",
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `accessTest`                                                                                                           | [operations.AccessTest4](../../models/operations/accesstest4.md)                                                       | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `availability`                                                                                                         | [operations.AvailabilityEnum4](../../models/operations/availabilityenum4.md)                                           | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `blockers`                                                                                                             | [operations.BlockerEnum4](../../models/operations/blockerenum4.md)[]                                                   | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `clientApis`                                                                                                           | [operations.GetResourceDeploymentDetailClientApi4](../../models/operations/getresourcedeploymentdetailclientapi4.md)[] | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `errorCode`                                                                                                            | *string*                                                                                                               | :heavy_minus_sign:                                                                                                     | N/A                                                                                                                    |
| `publicModelId`                                                                                                        | *string*                                                                                                               | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `testedAt`                                                                                                             | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)                          | :heavy_minus_sign:                                                                                                     | N/A                                                                                                                    |