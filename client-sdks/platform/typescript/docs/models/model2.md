# Model2

## Example Usage

```typescript
import { Model2 } from "@alienplatform/platform-api/models";

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

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `accessTest`                                                                                  | [models.AccessTest2](../models/accesstest2.md)                                                | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `availability`                                                                                | [models.AvailabilityEnum2](../models/availabilityenum2.md)                                    | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `blockers`                                                                                    | [models.BlockerEnum2](../models/blockerenum2.md)[]                                            | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `clientApis`                                                                                  | [models.ClientApi2](../models/clientapi2.md)[]                                                | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `errorCode`                                                                                   | *string*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `publicModelId`                                                                               | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `testedAt`                                                                                    | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | N/A                                                                                           |