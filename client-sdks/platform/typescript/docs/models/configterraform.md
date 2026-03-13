# ConfigTerraform

Configuration for the Terraform provider binary

## Example Usage

```typescript
import { ConfigTerraform } from "@aliendotdev/platform-api/models";

let value: ConfigTerraform = {
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