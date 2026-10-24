# UpdateProjectTerraform

Terraform package configuration. If null, Terraform packages will not be generated.

## Example Usage

```typescript
import { UpdateProjectTerraform } from "@alienplatform/platform-api/models";

let value: UpdateProjectTerraform = {
  enabled: true,
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `enabled`                                                            | *boolean*                                                            | :heavy_check_mark:                                                   | Whether Terraform package generation is enabled                      |
| `displayName`                                                        | *string*                                                             | :heavy_minus_sign:                                                   | Human-friendly application name shown in generated install artifacts |