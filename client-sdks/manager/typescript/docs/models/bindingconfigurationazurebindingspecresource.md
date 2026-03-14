# BindingConfigurationAzureBindingSpecResource

Azure-specific binding specification

## Example Usage

```typescript
import { BindingConfigurationAzureBindingSpecResource } from "@alienplatform/manager-api/models";

let value: BindingConfigurationAzureBindingSpecResource = {
  scope: "<value>",
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `scope`                                            | *string*                                           | :heavy_check_mark:                                 | Scope (subscription/resource group/resource level) |