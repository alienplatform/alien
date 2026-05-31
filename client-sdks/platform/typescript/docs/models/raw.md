# Raw

## Example Usage

```typescript
import { Raw } from "@alienplatform/platform-api/models";

let value: Raw = {
  body: "<value>",
  collectedAt: new Date("2025-01-29T23:35:36.058Z"),
  format: "json",
  source: "<value>",
  truncated: true,
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `body`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `collectedAt`                                                                                 | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `format`                                                                                      | [models.Format](../models/format.md)                                                          | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `source`                                                                                      | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `truncated`                                                                                   | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |