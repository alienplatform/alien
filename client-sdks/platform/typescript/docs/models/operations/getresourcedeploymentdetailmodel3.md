# GetResourceDeploymentDetailModel3

## Example Usage

```typescript
import { GetResourceDeploymentDetailModel3 } from "@alienplatform/platform-api/models/operations";

let value: GetResourceDeploymentDetailModel3 = {
  accessTest: "not-checked",
  availability: "unknown",
  blockers: [
    "deployment-required",
  ],
  clientApis: [],
  publicModelId: "<id>",
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `accessTest`                                                                                                           | [operations.AccessTest3](../../models/operations/accesstest3.md)                                                       | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `availability`                                                                                                         | [operations.AvailabilityEnum3](../../models/operations/availabilityenum3.md)                                           | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `blockers`                                                                                                             | [operations.BlockerEnum3](../../models/operations/blockerenum3.md)[]                                                   | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `clientApis`                                                                                                           | [operations.GetResourceDeploymentDetailClientApi3](../../models/operations/getresourcedeploymentdetailclientapi3.md)[] | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `errorCode`                                                                                                            | *string*                                                                                                               | :heavy_minus_sign:                                                                                                     | N/A                                                                                                                    |
| `publicModelId`                                                                                                        | *string*                                                                                                               | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `testedAt`                                                                                                             | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)                          | :heavy_minus_sign:                                                                                                     | N/A                                                                                                                    |