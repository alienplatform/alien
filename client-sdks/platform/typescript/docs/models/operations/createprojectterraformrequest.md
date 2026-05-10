# CreateProjectTerraformRequest

Terraform package configuration. If null, Terraform packages will not be generated.

## Example Usage

```typescript
import { CreateProjectTerraformRequest } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectTerraformRequest = {
  enabled: true,
};
```

## Fields

| Field                                           | Type                                            | Required                                        | Description                                     |
| ----------------------------------------------- | ----------------------------------------------- | ----------------------------------------------- | ----------------------------------------------- |
| `enabled`                                       | *boolean*                                       | :heavy_check_mark:                              | Whether Terraform package generation is enabled |