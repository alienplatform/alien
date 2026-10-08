# DeploymentEnvironmentInfoTest

Test platform environment information (mock)

## Example Usage

```typescript
import { DeploymentEnvironmentInfoTest } from "@alienplatform/platform-api/models";

let value: DeploymentEnvironmentInfoTest = {
  testId: "<id>",
  platform: "test",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `testId`                                                             | *string*                                                             | :heavy_check_mark:                                                   | Test identifier for this environment                                 |
| `platform`                                                           | [models.DeploymentPlatformTest](../models/deploymentplatformtest.md) | :heavy_check_mark:                                                   | N/A                                                                  |