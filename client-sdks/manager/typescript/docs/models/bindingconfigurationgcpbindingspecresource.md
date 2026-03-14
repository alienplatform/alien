# BindingConfigurationGcpBindingSpecResource

GCP-specific binding specification

## Example Usage

```typescript
import { BindingConfigurationGcpBindingSpecResource } from "@alienplatform/manager-api/models";

let value: BindingConfigurationGcpBindingSpecResource = {
  scope: "<value>",
};
```

## Fields

| Field                                            | Type                                             | Required                                         | Description                                      |
| ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ |
| `condition`                                      | [models.GcpCondition](../models/gcpcondition.md) | :heavy_minus_sign:                               | N/A                                              |
| `scope`                                          | *string*                                         | :heavy_check_mark:                               | Scope (project/resource level)                   |