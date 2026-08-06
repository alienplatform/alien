# SyncReconcileRequestModel3

## Example Usage

```typescript
import { SyncReconcileRequestModel3 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestModel3 = {
  accessTest: "failed",
  availability: "blocked",
  blockers: [],
  clientApis: [
    "open-ai-responses",
  ],
  publicModelId: "<id>",
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `accessTest`                                                                                       | [models.SyncReconcileRequestAccessTest3](../models/syncreconcilerequestaccesstest3.md)             | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `availability`                                                                                     | [models.SyncReconcileRequestAvailabilityEnum3](../models/syncreconcilerequestavailabilityenum3.md) | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `blockers`                                                                                         | [models.SyncReconcileRequestBlockerEnum3](../models/syncreconcilerequestblockerenum3.md)[]         | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `clientApis`                                                                                       | [models.SyncReconcileRequestClientApi3](../models/syncreconcilerequestclientapi3.md)[]             | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `errorCode`                                                                                        | *string*                                                                                           | :heavy_minus_sign:                                                                                 | N/A                                                                                                |
| `publicModelId`                                                                                    | *string*                                                                                           | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `testedAt`                                                                                         | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)      | :heavy_minus_sign:                                                                                 | N/A                                                                                                |