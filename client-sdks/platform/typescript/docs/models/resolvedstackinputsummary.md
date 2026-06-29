# ResolvedStackInputSummary

## Example Usage

```typescript
import { ResolvedStackInputSummary } from "@alienplatform/platform-api/models";

let value: ResolvedStackInputSummary = {
  id: "<id>",
  label: "<value>",
  providedBy: [
    "developer",
  ],
  required: true,
  secret: false,
  provided: true,
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `id`                                                                                             | *string*                                                                                         | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `label`                                                                                          | *string*                                                                                         | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `providedBy`                                                                                     | [models.ResolvedStackInputSummaryProvidedBy](../models/resolvedstackinputsummaryprovidedby.md)[] | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `required`                                                                                       | *boolean*                                                                                        | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `secret`                                                                                         | *boolean*                                                                                        | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `provided`                                                                                       | *boolean*                                                                                        | :heavy_check_mark:                                                                               | N/A                                                                                              |