# Model3

## Example Usage

```typescript
import { Model3 } from "@alienplatform/platform-api/models";

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
| `accessTest`                                                                                  | [models.AccessTest3](../models/accesstest3.md)                                                | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `availability`                                                                                | [models.AvailabilityEnum3](../models/availabilityenum3.md)                                    | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `blockers`                                                                                    | [models.BlockerEnum3](../models/blockerenum3.md)[]                                            | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `clientApis`                                                                                  | [models.ClientApi3](../models/clientapi3.md)[]                                                | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `errorCode`                                                                                   | *string*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `publicModelId`                                                                               | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `testedAt`                                                                                    | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | N/A                                                                                           |