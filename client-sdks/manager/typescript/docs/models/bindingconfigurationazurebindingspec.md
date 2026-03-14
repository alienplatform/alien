# BindingConfigurationAzureBindingSpec

Generic binding configuration for permissions

## Example Usage

```typescript
import { BindingConfigurationAzureBindingSpec } from "@alienplatform/manager-api/models";

let value: BindingConfigurationAzureBindingSpec = {};
```

## Fields

| Field                                                                                                            | Type                                                                                                             | Required                                                                                                         | Description                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `resource`                                                                                                       | [models.BindingConfigurationAzureBindingSpecResource](../models/bindingconfigurationazurebindingspecresource.md) | :heavy_minus_sign:                                                                                               | Azure-specific binding specification                                                                             |
| `stack`                                                                                                          | [models.BindingConfigurationAzureBindingSpecStack](../models/bindingconfigurationazurebindingspecstack.md)       | :heavy_minus_sign:                                                                                               | Azure-specific binding specification                                                                             |