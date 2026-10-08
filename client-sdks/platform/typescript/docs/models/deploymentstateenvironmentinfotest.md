# DeploymentStateEnvironmentInfoTest

Test platform environment information (mock)

## Example Usage

```typescript
import { DeploymentStateEnvironmentInfoTest } from "@alienplatform/platform-api/models";

let value: DeploymentStateEnvironmentInfoTest = {
  testId: "<id>",
  platform: "test",
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `testId`                                                                       | *string*                                                                       | :heavy_check_mark:                                                             | Test identifier for this environment                                           |
| `platform`                                                                     | [models.DeploymentStatePlatformTest](../models/deploymentstateplatformtest.md) | :heavy_check_mark:                                                             | N/A                                                                            |