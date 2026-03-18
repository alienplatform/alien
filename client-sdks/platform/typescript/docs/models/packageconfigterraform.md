# PackageConfigTerraform

## Example Usage

```typescript
import { PackageConfigTerraform } from "@alienplatform/platform-api/models";

let value: PackageConfigTerraform = {
  providerName: "<value>",
  resourceType: "<value>",
  type: "terraform",
};
```

## Fields

| Field                                        | Type                                         | Required                                     | Description                                  |
| -------------------------------------------- | -------------------------------------------- | -------------------------------------------- | -------------------------------------------- |
| `providerName`                               | *string*                                     | :heavy_check_mark:                           | Terraform provider name (e.g., "acme")       |
| `resourceType`                               | *string*                                     | :heavy_check_mark:                           | Terraform resource type name (e.g., "agent") |
| `type`                                       | *"terraform"*                                | :heavy_check_mark:                           | N/A                                          |