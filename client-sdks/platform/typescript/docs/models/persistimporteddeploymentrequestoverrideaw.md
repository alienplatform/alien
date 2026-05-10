# PersistImportedDeploymentRequestOverrideAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { PersistImportedDeploymentRequestOverrideAw } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestOverrideAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                  | [models.PersistImportedDeploymentRequestOverrideAwBinding](../models/persistimporteddeploymentrequestoverrideawbinding.md) | :heavy_check_mark:                                                                                                         | Generic binding configuration for permissions                                                                              |
| `effect`                                                                                                                   | [models.PersistImportedDeploymentRequestOverrideEffect](../models/persistimporteddeploymentrequestoverrideeffect.md)       | :heavy_minus_sign:                                                                                                         | IAM effect. Defaults to Allow.                                                                                             |
| `grant`                                                                                                                    | [models.PersistImportedDeploymentRequestOverrideAwGrant](../models/persistimporteddeploymentrequestoverrideawgrant.md)     | :heavy_check_mark:                                                                                                         | Grant permissions for a specific cloud platform                                                                            |