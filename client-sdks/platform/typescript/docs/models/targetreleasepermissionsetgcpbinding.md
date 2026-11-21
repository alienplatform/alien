# TargetReleasePermissionSetGcpBinding

Generic binding configuration for permissions

## Example Usage

```typescript
import { TargetReleasePermissionSetGcpBinding } from "@alienplatform/platform-api/models";

let value: TargetReleasePermissionSetGcpBinding = {};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `resource`                                                                                         | [models.TargetReleasePermissionSetGcpResource](../models/targetreleasepermissionsetgcpresource.md) | :heavy_minus_sign:                                                                                 | GCP-specific binding specification                                                                 |
| `stack`                                                                                            | [models.TargetReleasePermissionSetGcpStack](../models/targetreleasepermissionsetgcpstack.md)       | :heavy_minus_sign:                                                                                 | GCP-specific binding specification                                                                 |