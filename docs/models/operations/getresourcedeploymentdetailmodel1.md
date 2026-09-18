# GetResourceDeploymentDetailModel1

## Example Usage

```typescript
import { GetResourceDeploymentDetailModel1 } from "@alienplatform/platform-api/models/operations";

let value: GetResourceDeploymentDetailModel1 = {
  accessTest: "not-checked",
  availability: "available",
  blockers: [
    "agreement-required",
  ],
  clientApis: [
    "open-ai-chat-completions",
  ],
  publicModelId: "<id>",
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `accessTest`                                                                                                           | [operations.AccessTest1](../../models/operations/accesstest1.md)                                                       | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `availability`                                                                                                         | [operations.AvailabilityEnum1](../../models/operations/availabilityenum1.md)                                           | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `blockers`                                                                                                             | [operations.BlockerEnum1](../../models/operations/blockerenum1.md)[]                                                   | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `clientApis`                                                                                                           | [operations.GetResourceDeploymentDetailClientApi1](../../models/operations/getresourcedeploymentdetailclientapi1.md)[] | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `errorCode`                                                                                                            | *string*                                                                                                               | :heavy_minus_sign:                                                                                                     | N/A                                                                                                                    |
| `publicModelId`                                                                                                        | *string*                                                                                                               | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `testedAt`                                                                                                             | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)                          | :heavy_minus_sign:                                                                                                     | N/A                                                                                                                    |