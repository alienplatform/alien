# ExportValuesRequest

## Example Usage

```typescript
import { ExportValuesRequest } from "@alienplatform/manager-api/models";

let value: ExportValuesRequest = {
  slots: [
    "<value 1>",
  ],
  targetToken: "<value>",
  targetUrl: "https://outlying-antelope.com/",
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `slots`                                                                                                            | *string*[]                                                                                                         | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |
| `targetToken`                                                                                                      | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | Token the target accepts for writing these slots. Passed on as is.                                                 |
| `targetUrl`                                                                                                        | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | Base URL of the manager that takes the values. The caller's token names the one URL<br/>allowed; any other is refused. |
