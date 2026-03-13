# SyncReconcileResponseEnvironmentInfoTest

Test platform environment information (mock)

## Example Usage

```typescript
import { SyncReconcileResponseEnvironmentInfoTest } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseEnvironmentInfoTest = {
  testId: "<id>",
  platform: "test",
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `testId`                                                                                   | *string*                                                                                   | :heavy_check_mark:                                                                         | Test identifier for this environment                                                       |
| `platform`                                                                                 | [models.SyncReconcileResponsePlatformTest](../models/syncreconcileresponseplatformtest.md) | :heavy_check_mark:                                                                         | N/A                                                                                        |