# GetSetupEnrollmentLocationTerraform

## Example Usage

```typescript
import { GetSetupEnrollmentLocationTerraform } from "@alienplatform/platform-api/models/operations";

let value: GetSetupEnrollmentLocationTerraform = {
  kind: "terraform",
  cloud: "azure",
  resourceId: "<id>",
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `kind`                                                                                   | *"terraform"*                                                                            | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `cloud`                                                                                  | [operations.GetSetupEnrollmentCloud](../../models/operations/getsetupenrollmentcloud.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `resourceId`                                                                             | *string*                                                                                 | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `accountId`                                                                              | *string*                                                                                 | :heavy_minus_sign:                                                                       | N/A                                                                                      |
| `region`                                                                                 | *string*                                                                                 | :heavy_minus_sign:                                                                       | N/A                                                                                      |