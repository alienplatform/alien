# ProjectTerraform

Terraform package configuration. If null, Terraform packages will not be generated.

## Example Usage

```typescript
import { ProjectTerraform } from "@alienplatform/platform-api/models";

let value: ProjectTerraform = {
  enabled: true,
};
```

## Fields

| Field                                           | Type                                            | Required                                        | Description                                     |
| ----------------------------------------------- | ----------------------------------------------- | ----------------------------------------------- | ----------------------------------------------- |
| `enabled`                                       | *boolean*                                       | :heavy_check_mark:                              | Whether Terraform package generation is enabled |