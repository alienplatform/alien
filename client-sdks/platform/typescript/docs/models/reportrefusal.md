# ReportRefusal

Why the installed chart's setup report was declined, when it was.

## Example Usage

```typescript
import { ReportRefusal } from "@alienplatform/platform-api/models";

let value: ReportRefusal = {
  at: new Date("2024-12-10T21:41:01.076Z"),
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