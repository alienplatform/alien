# SyncReconcileRequestModel1

## Example Usage

```typescript
import { SyncReconcileRequestModel1 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestModel1 = {
  accessTest: "failed",
  availability: "available",
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

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `accessTest`                                                                                         | [models.SyncReconcileRequestAccessTest1](../models/syncreconcilerequestaccesstest1.md)               | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `availability`                                                                                       | [models.SyncReconcileRequestModelAvailability1](../models/syncreconcilerequestmodelavailability1.md) | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `blockers`                                                                                           | [models.SyncReconcileRequestBlockerEnum1](../models/syncreconcilerequestblockerenum1.md)[]           | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `clientApis`                                                                                         | [models.SyncReconcileRequestClientApi1](../models/syncreconcilerequestclientapi1.md)[]               | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `errorCode`                                                                                          | *string*                                                                                             | :heavy_minus_sign:                                                                                   | N/A                                                                                                  |
| `publicModelId`                                                                                      | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `testedAt`                                                                                           | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)        | :heavy_minus_sign:                                                                                   | N/A                                                                                                  |