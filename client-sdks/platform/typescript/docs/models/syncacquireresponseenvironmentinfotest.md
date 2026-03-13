# SyncAcquireResponseEnvironmentInfoTest

Test platform environment information (mock)

## Example Usage

```typescript
import { SyncAcquireResponseEnvironmentInfoTest } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseEnvironmentInfoTest = {
  testId: "<id>",
  platform: "test",
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `testId`                                                                               | *string*                                                                               | :heavy_check_mark:                                                                     | Test identifier for this environment                                                   |
| `platform`                                                                             | [models.SyncAcquireResponsePlatformTest](../models/syncacquireresponseplatformtest.md) | :heavy_check_mark:                                                                     | N/A                                                                                    |