# Model1

## Example Usage

```typescript
import { Model1 } from "@alienplatform/platform-api/models";

let value: Model1 = {
  accessTest: "failed",
  availability: "unknown",
  blockers: [],
  clientApis: [],
  publicModelId: "<id>",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `accessTest`                                                                                  | [models.AccessTest1](../models/accesstest1.md)                                                | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `availability`                                                                                | [models.AvailabilityEnum1](../models/availabilityenum1.md)                                    | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `blockers`                                                                                    | [models.BlockerEnum1](../models/blockerenum1.md)[]                                            | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `clientApis`                                                                                  | [models.ClientApi1](../models/clientapi1.md)[]                                                | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `errorCode`                                                                                   | *string*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `publicModelId`                                                                               | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `testedAt`                                                                                    | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | N/A                                                                                           |