# ExternalAIModelCheck

## Example Usage

```typescript
import { ExternalAIModelCheck } from "@alienplatform/platform-api/models";

let value: ExternalAIModelCheck = {
  id: "<id>",
  publicModel: "<value>",
  status: "running",
  blockerCode: null,
  requestedAt: new Date("2025-10-28T21:29:51.299Z"),
  completedAt: null,
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `id`                                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `publicModel`                                                                                 | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `status`                                                                                      | [models.ExternalAIModelCheckStatus](../models/externalaimodelcheckstatus.md)                  | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `blockerCode`                                                                                 | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `error`                                                                                       | *any*                                                                                         | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `requestedAt`                                                                                 | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `completedAt`                                                                                 | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |