# SyncReconcileRequestModel2

## Example Usage

```typescript
import { SyncReconcileRequestModel2 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestModel2 = {
  accessTest: "failed",
  availability: "blocked",
  blockers: [
    "deployment-required",
  ],
  clientApis: [
    "open-ai-responses",
  ],
  publicModelId: "<id>",
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `accessTest`                                                                                         | [models.SyncReconcileRequestAccessTest2](../models/syncreconcilerequestaccesstest2.md)               | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `availability`                                                                                       | [models.SyncReconcileRequestModelAvailability2](../models/syncreconcilerequestmodelavailability2.md) | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `blockers`                                                                                           | [models.AvailabilityBlocker2](../models/availabilityblocker2.md)[]                                   | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `clientApis`                                                                                         | [models.SyncReconcileRequestClientApi2](../models/syncreconcilerequestclientapi2.md)[]               | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `errorCode`                                                                                          | *string*                                                                                             | :heavy_minus_sign:                                                                                   | N/A                                                                                                  |
| `publicModelId`                                                                                      | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `testedAt`                                                                                           | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)        | :heavy_minus_sign:                                                                                   | N/A                                                                                                  |