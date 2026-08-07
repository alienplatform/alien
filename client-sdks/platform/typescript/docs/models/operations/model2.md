# Model2

## Example Usage

```typescript
import { Model2 } from "@alienplatform/platform-api/models/operations";

let value: Model2 = {
  accessTest: "failed",
  availability: "available",
  blockers: [],
  clientApis: [
    "anthropic-messages",
  ],
  publicModelId: "<id>",
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `accessTest`                                                                                                           | [operations.AccessTest2](../../models/operations/accesstest2.md)                                                       | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `availability`                                                                                                         | [operations.AvailabilityEnum2](../../models/operations/availabilityenum2.md)                                           | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `blockers`                                                                                                             | [operations.BlockerEnum2](../../models/operations/blockerenum2.md)[]                                                   | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `clientApis`                                                                                                           | [operations.GetResourceDeploymentDetailClientApi2](../../models/operations/getresourcedeploymentdetailclientapi2.md)[] | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `errorCode`                                                                                                            | *string*                                                                                                               | :heavy_minus_sign:                                                                                                     | N/A                                                                                                                    |
| `publicModelId`                                                                                                        | *string*                                                                                                               | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `testedAt`                                                                                                             | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)                          | :heavy_minus_sign:                                                                                                     | N/A                                                                                                                    |