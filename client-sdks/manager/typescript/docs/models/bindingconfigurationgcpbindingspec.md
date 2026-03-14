# BindingConfigurationGcpBindingSpec

Generic binding configuration for permissions

## Example Usage

```typescript
import { BindingConfigurationGcpBindingSpec } from "@alienplatform/manager-api/models";

let value: BindingConfigurationGcpBindingSpec = {};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `resource`                                                                                                   | [models.BindingConfigurationGcpBindingSpecResource](../models/bindingconfigurationgcpbindingspecresource.md) | :heavy_minus_sign:                                                                                           | GCP-specific binding specification                                                                           |
| `stack`                                                                                                      | [models.BindingConfigurationGcpBindingSpecStack](../models/bindingconfigurationgcpbindingspecstack.md)       | :heavy_minus_sign:                                                                                           | GCP-specific binding specification                                                                           |