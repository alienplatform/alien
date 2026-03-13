# NewDeploymentRequestEnvironmentInfoTest

Test platform environment information (mock)

## Example Usage

```typescript
import { NewDeploymentRequestEnvironmentInfoTest } from "@alienplatform/platform-api/models";

let value: NewDeploymentRequestEnvironmentInfoTest = {
  testId: "<id>",
  platform: "test",
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `testId`                                                                                 | *string*                                                                                 | :heavy_check_mark:                                                                       | Test identifier for this environment                                                     |
| `platform`                                                                               | [models.NewDeploymentRequestPlatformTest](../models/newdeploymentrequestplatformtest.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |