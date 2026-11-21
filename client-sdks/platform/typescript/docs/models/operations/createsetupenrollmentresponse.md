# CreateSetupEnrollmentResponse

One-use setup enrollment.

## Example Usage

```typescript
import { CreateSetupEnrollmentResponse } from "@alienplatform/platform-api/models/operations";

let value: CreateSetupEnrollmentResponse = {
  code: "<value>",
  expiresAt: new Date("2024-10-08T20:49:54.411Z"),
  revision: 932277,
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `cloudFormationLaunchUrl`                                                                     | *string*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `managementExternalId`                                                                        | *string*                                                                                      | :heavy_minus_sign:                                                                            | The AWS ExternalId a verified Terraform module passes to its management role.                 |
| `code`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `expiresAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `revision`                                                                                    | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |