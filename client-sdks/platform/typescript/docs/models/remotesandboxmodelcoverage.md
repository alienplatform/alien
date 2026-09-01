# RemoteSandboxModelCoverage

## Example Usage

```typescript
import { RemoteSandboxModelCoverage } from "@alienplatform/platform-api/models";

let value: RemoteSandboxModelCoverage = {
  publicModelId: "<id>",
  required: false,
  availability: "unknown",
  blockerCodes: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  accessTest: "not-checked",
  accessObservedAt: new Date("2025-06-13T18:01:13.000Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `publicModelId`                                                                               | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `required`                                                                                    | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `availability`                                                                                | [models.RemoteSandboxAvailability](../models/remotesandboxavailability.md)                    | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `blockerCodes`                                                                                | *string*[]                                                                                    | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `accessTest`                                                                                  | [models.RemoteSandboxAccessTest](../models/remotesandboxaccesstest.md)                        | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `accessObservedAt`                                                                            | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |