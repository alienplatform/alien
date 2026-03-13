# DeploymentProfileAzureStack

Azure-specific binding specification

## Example Usage

```typescript
import { DeploymentProfileAzureStack } from "@alienplatform/platform-api/models";

let value: DeploymentProfileAzureStack = {
  scope: "<value>",
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `scope`                                            | *string*                                           | :heavy_check_mark:                                 | Scope (subscription/resource group/resource level) |