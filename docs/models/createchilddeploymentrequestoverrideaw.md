# CreateChildDeploymentRequestOverrideAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { CreateChildDeploymentRequestOverrideAw } from "@alienplatform/platform-api/models";

let value: CreateChildDeploymentRequestOverrideAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                          | [models.CreateChildDeploymentRequestOverrideAwBinding](../models/createchilddeploymentrequestoverrideawbinding.md) | :heavy_check_mark:                                                                                                 | Generic binding configuration for permissions                                                                      |
| `description`                                                                                                      | *string*                                                                                                           | :heavy_minus_sign:                                                                                                 | Short admin-facing description of why this entry exists.                                                           |
| `effect`                                                                                                           | [models.CreateChildDeploymentRequestOverrideEffect](../models/createchilddeploymentrequestoverrideeffect.md)       | :heavy_minus_sign:                                                                                                 | IAM effect. Defaults to Allow.                                                                                     |
| `grant`                                                                                                            | [models.CreateChildDeploymentRequestOverrideAwGrant](../models/createchilddeploymentrequestoverrideawgrant.md)     | :heavy_check_mark:                                                                                                 | Grant permissions for a specific cloud platform                                                                    |
| `label`                                                                                                            | *string*                                                                                                           | :heavy_minus_sign:                                                                                                 | Stable admin-facing label for this permission entry.                                                               |