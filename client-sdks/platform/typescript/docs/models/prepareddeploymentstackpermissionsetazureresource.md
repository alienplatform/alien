# PreparedDeploymentStackPermissionSetAzureResource

Azure-specific binding specification

## Example Usage

```typescript
import { PreparedDeploymentStackPermissionSetAzureResource } from "@alienplatform/platform-api/models";

let value: PreparedDeploymentStackPermissionSetAzureResource = {
  scope: "<value>",
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `scope`                                            | *string*                                           | :heavy_check_mark:                                 | Scope (subscription/resource group/resource level) |