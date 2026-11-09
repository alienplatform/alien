# IssuedEnrollmentCode

A one-hour, one-use code that enrolls the deployment's operator.

## Example Usage

```typescript
import { IssuedEnrollmentCode } from "@alienplatform/manager-api/models";

let value: IssuedEnrollmentCode = {
  code: "<value>",
  deploymentId: "<id>",
  expiresAt: new Date("2025-08-27T21:23:55.628Z"),
  managerAudience: "<value>",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `code`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `deploymentId`                                                                                | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `expiresAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `managerAudience`                                                                             | *string*                                                                                      | :heavy_check_mark:                                                                            | The audience the operator must use for this manager.                                          |
