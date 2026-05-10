# UpdateProjectTerraform

Terraform package configuration. If null, Terraform packages will not be generated.

## Example Usage

```typescript
import { UpdateProjectTerraform } from "@alienplatform/platform-api/models/operations";

let value: UpdateProjectTerraform = {
  enabled: true,
};
```

## Fields

| Field                                           | Type                                            | Required                                        | Description                                     |
| ----------------------------------------------- | ----------------------------------------------- | ----------------------------------------------- | ----------------------------------------------- |
| `enabled`                                       | *boolean*                                       | :heavy_check_mark:                              | Whether Terraform package generation is enabled |