# RegistryModelCoverage

## Example Usage

```typescript
import { RegistryModelCoverage } from "@alienplatform/platform-api/models";

let value: RegistryModelCoverage = {
  publicModelId: "<id>",
  required: false,
  availability: "unknown",
  blockerCodes: [
    "<value 1>",
    "<value 2>",
  ],
  accessTest: "failed",
  accessObservedAt: new Date("2025-09-17T09:26:59.262Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `publicModelId`                                                                               | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `required`                                                                                    | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `availability`                                                                                | [models.RegistryAvailability](../models/registryavailability.md)                              | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `blockerCodes`                                                                                | *string*[]                                                                                    | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `accessTest`                                                                                  | [models.RegistryAccessTest](../models/registryaccesstest.md)                                  | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `accessObservedAt`                                                                            | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |