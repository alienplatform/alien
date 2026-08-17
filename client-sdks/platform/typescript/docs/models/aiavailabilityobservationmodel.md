# AiAvailabilityObservationModel

## Example Usage

```typescript
import { AiAvailabilityObservationModel } from "@alienplatform/platform-api/models";

let value: AiAvailabilityObservationModel = {
  accessTest: "failed",
  availability: "available",
  blockers: [
    "access-denied",
  ],
  clientApis: [],
  publicModelId: "<id>",
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `accessTest`                                                                                       | [models.AiAvailabilityObservationAccessTest](../models/aiavailabilityobservationaccesstest.md)     | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `availability`                                                                                     | [models.AiAvailabilityObservationAvailability](../models/aiavailabilityobservationavailability.md) | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `blockers`                                                                                         | [models.AiAvailabilityObservationBlocker](../models/aiavailabilityobservationblocker.md)[]         | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `clientApis`                                                                                       | [models.AiAvailabilityObservationClientApi](../models/aiavailabilityobservationclientapi.md)[]     | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `errorCode`                                                                                        | *string*                                                                                           | :heavy_minus_sign:                                                                                 | N/A                                                                                                |
| `publicModelId`                                                                                    | *string*                                                                                           | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `testedAt`                                                                                         | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)      | :heavy_minus_sign:                                                                                 | N/A                                                                                                |