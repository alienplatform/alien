# CreateSetupEnrollmentLocationTerraform

## Example Usage

```typescript
import { CreateSetupEnrollmentLocationTerraform } from "@alienplatform/platform-api/models/operations";

let value: CreateSetupEnrollmentLocationTerraform = {
  kind: "terraform",
  cloud: "aws",
  resourceId: "<id>",
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `kind`                                                                                         | *"terraform"*                                                                                  | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `cloud`                                                                                        | [operations.CreateSetupEnrollmentCloud](../../models/operations/createsetupenrollmentcloud.md) | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `resourceId`                                                                                   | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `accountId`                                                                                    | *string*                                                                                       | :heavy_minus_sign:                                                                             | N/A                                                                                            |
| `region`                                                                                       | *string*                                                                                       | :heavy_minus_sign:                                                                             | N/A                                                                                            |