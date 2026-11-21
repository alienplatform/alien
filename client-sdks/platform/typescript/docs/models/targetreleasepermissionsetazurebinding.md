# TargetReleasePermissionSetAzureBinding

Generic binding configuration for permissions

## Example Usage

```typescript
import { TargetReleasePermissionSetAzureBinding } from "@alienplatform/platform-api/models";

let value: TargetReleasePermissionSetAzureBinding = {};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `resource`                                                                                             | [models.TargetReleasePermissionSetAzureResource](../models/targetreleasepermissionsetazureresource.md) | :heavy_minus_sign:                                                                                     | Azure-specific binding specification                                                                   |
| `stack`                                                                                                | [models.TargetReleasePermissionSetAzureStack](../models/targetreleasepermissionsetazurestack.md)       | :heavy_minus_sign:                                                                                     | Azure-specific binding specification                                                                   |