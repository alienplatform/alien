# SyncReconcileRequestModel4

## Example Usage

```typescript
import { SyncReconcileRequestModel4 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestModel4 = {
  accessTest: "not-checked",
  availability: "unknown",
  blockers: [],
  clientApis: [
    "open-ai-responses",
  ],
  publicModelId: "<id>",
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `accessTest`                                                                                         | [models.SyncReconcileRequestAccessTest4](../models/syncreconcilerequestaccesstest4.md)               | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `availability`                                                                                       | [models.SyncReconcileRequestModelAvailability4](../models/syncreconcilerequestmodelavailability4.md) | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `blockers`                                                                                           | [models.AvailabilityBlocker4](../models/availabilityblocker4.md)[]                                   | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `clientApis`                                                                                         | [models.SyncReconcileRequestClientApi4](../models/syncreconcilerequestclientapi4.md)[]               | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `errorCode`                                                                                          | *string*                                                                                             | :heavy_minus_sign:                                                                                   | N/A                                                                                                  |
| `publicModelId`                                                                                      | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `testedAt`                                                                                           | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)        | :heavy_minus_sign:                                                                                   | N/A                                                                                                  |