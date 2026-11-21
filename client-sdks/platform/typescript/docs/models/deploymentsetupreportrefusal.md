# DeploymentSetupReportRefusal

A setup report the manager declined, for display next to the waiting update.

## Example Usage

```typescript
import { DeploymentSetupReportRefusal } from "@alienplatform/platform-api/models";

let value: DeploymentSetupReportRefusal = {
  at: new Date("2024-10-05T13:40:26.087Z"),
  reason: "<value>",
  reportedReleaseId: "<id>",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `at`                                                                                          | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | When the manager declined it.                                                                 |
| `reason`                                                                                      | *string*                                                                                      | :heavy_check_mark:                                                                            | Plain-language reason, safe to show to the customer.                                          |
| `reportedReleaseId`                                                                           | *string*                                                                                      | :heavy_check_mark:                                                                            | Release the chart reported.                                                                   |