# AiModelAvailabilityObservation

## Example Usage

```typescript
import { AiModelAvailabilityObservation } from "@alienplatform/manager-api/models";

let value: AiModelAvailabilityObservation = {
  accessTest: "verified",
  availability: "blocked",
  blockers: [
    "quota-configuration-required",
  ],
  clientApis: [
    "open-ai-chat-completions",
  ],
  publicModelId: "<id>",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `accessTest`                                                                                  | [models.AiAccessTest](../models/aiaccesstest.md)                                              | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `availability`                                                                                | [models.AiModelAvailability](../models/aimodelavailability.md)                                | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `blockers`                                                                                    | [models.AiAvailabilityBlocker](../models/aiavailabilityblocker.md)[]                          | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `clientApis`                                                                                  | [models.ClientApi](../models/clientapi.md)[]                                                  | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `errorCode`                                                                                   | *string*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `publicModelId`                                                                               | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `testedAt`                                                                                    | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | N/A                                                                                           |