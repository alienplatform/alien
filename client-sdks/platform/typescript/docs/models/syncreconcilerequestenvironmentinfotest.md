# SyncReconcileRequestEnvironmentInfoTest

Test platform environment information (mock)

## Example Usage

```typescript
import { SyncReconcileRequestEnvironmentInfoTest } from "@aliendotdev/platform-api/models";

let value: SyncReconcileRequestEnvironmentInfoTest = {
  testId: "<id>",
  platform: "test",
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `testId`                                                                                 | *string*                                                                                 | :heavy_check_mark:                                                                       | Test identifier for this environment                                                     |
| `platform`                                                                               | [models.SyncReconcileRequestPlatformTest](../models/syncreconcilerequestplatformtest.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |