# CommandDeploymentInfoEnvironmentInfoTest

Test platform environment information (mock)

## Example Usage

```typescript
import { CommandDeploymentInfoEnvironmentInfoTest } from "@alienplatform/platform-api/models";

let value: CommandDeploymentInfoEnvironmentInfoTest = {
  testId: "<id>",
  platform: "test",
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `testId`                                                                                   | *string*                                                                                   | :heavy_check_mark:                                                                         | Test identifier for this environment                                                       |
| `platform`                                                                                 | [models.CommandDeploymentInfoPlatformTest](../models/commanddeploymentinfoplatformtest.md) | :heavy_check_mark:                                                                         | N/A                                                                                        |