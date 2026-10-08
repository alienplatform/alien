# ModelsModelCoverage

## Example Usage

```typescript
import { ModelsModelCoverage } from "@alienplatform/platform-api/models";

let value: ModelsModelCoverage = {
  publicModelId: "<id>",
  required: false,
  availability: "available",
  blockerCodes: [
    "<value 1>",
    "<value 2>",
  ],
  accessTest: "verified",
  accessObservedAt: null,
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `publicModelId`                                                                               | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `required`                                                                                    | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `availability`                                                                                | [models.ModelsAvailability](../models/modelsavailability.md)                                  | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `blockerCodes`                                                                                | *string*[]                                                                                    | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `accessTest`                                                                                  | [models.ModelsAccessTest](../models/modelsaccesstest.md)                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `accessObservedAt`                                                                            | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |