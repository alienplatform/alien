# ProjectListItemResponseTerraform

Terraform provider package configuration. If null, Terraform packages will not be generated.

## Example Usage

```typescript
import { ProjectListItemResponseTerraform } from "@alienplatform/platform-api/models";

let value: ProjectListItemResponseTerraform = {
  providerName: "<value>",
  resourceType: "<value>",
  enabled: false,
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `providerName`                                           | *string*                                                 | :heavy_check_mark:                                       | Terraform provider name (e.g., "acme")                   |
| `resourceType`                                           | *string*                                                 | :heavy_check_mark:                                       | Terraform resource type name (e.g., "agent")             |
| `enabled`                                                | *boolean*                                                | :heavy_check_mark:                                       | Whether Terraform provider package generation is enabled |