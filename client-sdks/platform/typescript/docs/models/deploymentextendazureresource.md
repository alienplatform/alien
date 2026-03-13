# DeploymentExtendAzureResource

Azure-specific binding specification

## Example Usage

```typescript
import { DeploymentExtendAzureResource } from "@aliendotdev/platform-api/models";

let value: DeploymentExtendAzureResource = {
  scope: "<value>",
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `scope`                                            | *string*                                           | :heavy_check_mark:                                 | Scope (subscription/resource group/resource level) |