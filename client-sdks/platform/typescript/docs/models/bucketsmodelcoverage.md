# BucketsModelCoverage

## Example Usage

```typescript
import { BucketsModelCoverage } from "@alienplatform/platform-api/models";

let value: BucketsModelCoverage = {
  publicModelId: "<id>",
  required: false,
  availability: "unknown",
  blockerCodes: [
    "<value 1>",
  ],
  accessTest: "not-checked",
  accessObservedAt: new Date("2026-09-11T20:48:47.165Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `publicModelId`                                                                               | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `required`                                                                                    | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `availability`                                                                                | [models.BucketsAvailability](../models/bucketsavailability.md)                                | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `blockerCodes`                                                                                | *string*[]                                                                                    | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `accessTest`                                                                                  | [models.BucketsAccessTest](../models/bucketsaccesstest.md)                                    | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `accessObservedAt`                                                                            | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |