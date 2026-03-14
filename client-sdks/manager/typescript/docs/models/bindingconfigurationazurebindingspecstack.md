# BindingConfigurationAzureBindingSpecStack

Azure-specific binding specification

## Example Usage

```typescript
import { BindingConfigurationAzureBindingSpecStack } from "@alienplatform/manager-api/models";

let value: BindingConfigurationAzureBindingSpecStack = {
  scope: "<value>",
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `scope`                                            | *string*                                           | :heavy_check_mark:                                 | Scope (subscription/resource group/resource level) |