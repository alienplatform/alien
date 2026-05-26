# ConfigTerraform

Configuration for Terraform package generation.

## Example Usage

```typescript
import { ConfigTerraform } from "@alienplatform/platform-api/models";

let value: ConfigTerraform = {
  type: "terraform",
};
```

## Fields

| Field                                                                 | Type                                                                  | Required                                                              | Description                                                           |
| --------------------------------------------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------- |
| `displayName`                                                         | *string*                                                              | :heavy_minus_sign:                                                    | Human-friendly application name shown in generated install artifacts. |
| `type`                                                                | *"terraform"*                                                         | :heavy_check_mark:                                                    | N/A                                                                   |