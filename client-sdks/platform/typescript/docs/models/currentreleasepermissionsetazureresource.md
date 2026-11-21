# CurrentReleasePermissionSetAzureResource

Azure-specific binding specification

## Example Usage

```typescript
import { CurrentReleasePermissionSetAzureResource } from "@alienplatform/platform-api/models";

let value: CurrentReleasePermissionSetAzureResource = {
  scope: "<value>",
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `scope`                                            | *string*                                           | :heavy_check_mark:                                 | Scope (subscription/resource group/resource level) |