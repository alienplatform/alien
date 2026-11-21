# CurrentReleasePermissionSetGcpBinding

Generic binding configuration for permissions

## Example Usage

```typescript
import { CurrentReleasePermissionSetGcpBinding } from "@alienplatform/platform-api/models";

let value: CurrentReleasePermissionSetGcpBinding = {};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `resource`                                                                                           | [models.CurrentReleasePermissionSetGcpResource](../models/currentreleasepermissionsetgcpresource.md) | :heavy_minus_sign:                                                                                   | GCP-specific binding specification                                                                   |
| `stack`                                                                                              | [models.CurrentReleasePermissionSetGcpStack](../models/currentreleasepermissionsetgcpstack.md)       | :heavy_minus_sign:                                                                                   | GCP-specific binding specification                                                                   |