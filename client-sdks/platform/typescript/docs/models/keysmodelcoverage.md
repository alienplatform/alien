# KeysModelCoverage

## Example Usage

```typescript
import { KeysModelCoverage } from "@alienplatform/platform-api/models";

let value: KeysModelCoverage = {
  publicModelId: "<id>",
  required: false,
  availability: "available",
  blockerCodes: [
    "<value 1>",
  ],
  accessTest: "not-checked",
  accessObservedAt: new Date("2025-04-20T21:49:17.979Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `publicModelId`                                                                               | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `required`                                                                                    | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `availability`                                                                                | [models.KeysAvailability](../models/keysavailability.md)                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `blockerCodes`                                                                                | *string*[]                                                                                    | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `accessTest`                                                                                  | [models.KeysAccessTest](../models/keysaccesstest.md)                                          | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `accessObservedAt`                                                                            | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |