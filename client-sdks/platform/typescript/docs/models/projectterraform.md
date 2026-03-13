# ProjectTerraform

Terraform provider package configuration. If null, Terraform packages will not be generated.

## Example Usage

```typescript
import { ProjectTerraform } from "@alienplatform/platform-api/models";

let value: ProjectTerraform = {
  providerName: "<value>",
  resourceType: "<value>",
  enabled: true,
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `providerName`                                           | *string*                                                 | :heavy_check_mark:                                       | Terraform provider name (e.g., "acme")                   |
| `resourceType`                                           | *string*                                                 | :heavy_check_mark:                                       | Terraform resource type name (e.g., "agent")             |
| `enabled`                                                | *boolean*                                                | :heavy_check_mark:                                       | Whether Terraform provider package generation is enabled |