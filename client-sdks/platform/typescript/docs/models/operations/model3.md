# Model3

## Example Usage

```typescript
import { Model3 } from "@alienplatform/platform-api/models/operations";

let value: Model3 = {
  accessTest: "verified",
  availability: "blocked",
  blockers: [
    "model-activation-required",
  ],
  clientApis: [],
  publicModelId: "<id>",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `accessTest`                                                                                  | [operations.AccessTest3](../../models/operations/accesstest3.md)                              | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `availability`                                                                                | [operations.AvailabilityEnum3](../../models/operations/availabilityenum3.md)                  | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `blockers`                                                                                    | [operations.BlockerEnum3](../../models/operations/blockerenum3.md)[]                          | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `clientApis`                                                                                  | [operations.ClientApi3](../../models/operations/clientapi3.md)[]                              | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `errorCode`                                                                                   | *string*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `publicModelId`                                                                               | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `testedAt`                                                                                    | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | N/A                                                                                           |