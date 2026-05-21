# PreparedDeploymentStackOverrideAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { PreparedDeploymentStackOverrideAw } from "@alienplatform/platform-api/models";

let value: PreparedDeploymentStackOverrideAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                | [models.PreparedDeploymentStackOverrideAwBinding](../models/prepareddeploymentstackoverrideawbinding.md) | :heavy_check_mark:                                                                                       | Generic binding configuration for permissions                                                            |
| `effect`                                                                                                 | [models.PreparedDeploymentStackOverrideEffect](../models/prepareddeploymentstackoverrideeffect.md)       | :heavy_minus_sign:                                                                                       | IAM effect. Defaults to Allow.                                                                           |
| `grant`                                                                                                  | [models.PreparedDeploymentStackOverrideAwGrant](../models/prepareddeploymentstackoverrideawgrant.md)     | :heavy_check_mark:                                                                                       | Grant permissions for a specific cloud platform                                                          |