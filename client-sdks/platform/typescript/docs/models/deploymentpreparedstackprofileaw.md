# DeploymentPreparedStackProfileAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { DeploymentPreparedStackProfileAw } from "@alienplatform/platform-api/models";

let value: DeploymentPreparedStackProfileAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                              | [models.DeploymentPreparedStackProfileAwBinding](../models/deploymentpreparedstackprofileawbinding.md) | :heavy_check_mark:                                                                                     | Generic binding configuration for permissions                                                          |
| `description`                                                                                          | *string*                                                                                               | :heavy_minus_sign:                                                                                     | Short admin-facing description of why this entry exists.                                               |
| `effect`                                                                                               | [models.DeploymentPreparedStackProfileEffect](../models/deploymentpreparedstackprofileeffect.md)       | :heavy_minus_sign:                                                                                     | IAM effect. Defaults to Allow.                                                                         |
| `grant`                                                                                                | [models.DeploymentPreparedStackProfileAwGrant](../models/deploymentpreparedstackprofileawgrant.md)     | :heavy_check_mark:                                                                                     | Grant permissions for a specific cloud platform                                                        |
| `label`                                                                                                | *string*                                                                                               | :heavy_minus_sign:                                                                                     | Stable admin-facing label for this permission entry.                                                   |
