# ReleaseDeploymentItemEnvironmentInfoTest

Test platform environment information (mock)

## Example Usage

```typescript
import { ReleaseDeploymentItemEnvironmentInfoTest } from "@alienplatform/platform-api/models";

let value: ReleaseDeploymentItemEnvironmentInfoTest = {
  testId: "<id>",
  platform: "test",
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `testId`                                                                                   | *string*                                                                                   | :heavy_check_mark:                                                                         | Test identifier for this environment                                                       |
| `platform`                                                                                 | [models.ReleaseDeploymentItemPlatformTest](../models/releasedeploymentitemplatformtest.md) | :heavy_check_mark:                                                                         | N/A                                                                                        |