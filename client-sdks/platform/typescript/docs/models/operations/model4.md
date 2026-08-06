# Model4

## Example Usage

```typescript
import { Model4 } from "@alienplatform/platform-api/models/operations";

let value: Model4 = {
  accessTest: "failed",
  availability: "available",
  blockers: [],
  clientApis: [],
  publicModelId: "<id>",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `accessTest`                                                                                  | [operations.AccessTest4](../../models/operations/accesstest4.md)                              | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `availability`                                                                                | [operations.AvailabilityEnum4](../../models/operations/availabilityenum4.md)                  | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `blockers`                                                                                    | [operations.BlockerEnum4](../../models/operations/blockerenum4.md)[]                          | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `clientApis`                                                                                  | [operations.ClientApi4](../../models/operations/clientapi4.md)[]                              | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `errorCode`                                                                                   | *string*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `publicModelId`                                                                               | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `testedAt`                                                                                    | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | N/A                                                                                           |