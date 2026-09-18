# TargetDeploymentProfileAzureResource

Azure-specific binding specification

## Example Usage

```typescript
import { TargetDeploymentProfileAzureResource } from "@alienplatform/platform-api/models";

let value: TargetDeploymentProfileAzureResource = {
  scope: "<value>",
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `scope`                                            | *string*                                           | :heavy_check_mark:                                 | Scope (subscription/resource group/resource level) |