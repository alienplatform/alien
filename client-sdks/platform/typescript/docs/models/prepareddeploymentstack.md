# PreparedDeploymentStack

## Example Usage

```typescript
import { PreparedDeploymentStack } from "@alienplatform/platform-api/models";

let value: PreparedDeploymentStack = {
  platform: "aws",
  stack: {
    id: "<id>",
    resources: {},
  },
  setup: {
    target: "<value>",
    fingerprint: "<value>",
    version: 547972,
  },
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `platform`                                                                             | [models.PreparedDeploymentStackPlatform](../models/prepareddeploymentstackplatform.md) | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `stack`                                                                                | [models.PreparedDeploymentStackStack](../models/prepareddeploymentstackstack.md)       | :heavy_check_mark:                                                                     | A bag of resources, unaware of any cloud.                                              |
| `setup`                                                                                | [models.SetupFingerprintInfo](../models/setupfingerprintinfo.md)                       | :heavy_check_mark:                                                                     | N/A                                                                                    |