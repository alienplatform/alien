# Model4

## Example Usage

```typescript
import { Model4 } from "@alienplatform/platform-api/models";

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
| `accessTest`                                                                                  | [models.AccessTest4](../models/accesstest4.md)                                                | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `availability`                                                                                | [models.AvailabilityEnum4](../models/availabilityenum4.md)                                    | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `blockers`                                                                                    | [models.BlockerEnum4](../models/blockerenum4.md)[]                                            | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `clientApis`                                                                                  | [models.ClientApi4](../models/clientapi4.md)[]                                                | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `errorCode`                                                                                   | *string*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `publicModelId`                                                                               | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `testedAt`                                                                                    | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | N/A                                                                                           |