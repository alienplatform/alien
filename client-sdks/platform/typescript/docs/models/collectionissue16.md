# CollectionIssue16

## Example Usage

```typescript
import { CollectionIssue16 } from "@alienplatform/platform-api/models";

let value: CollectionIssue16 = {
  message: "<value>",
  reason: "forbidden",
  severity: "error",
  source: "<value>",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `message`                                                                  | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `reason`                                                                   | [models.Reason16](../models/reason16.md)                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `severity`                                                                 | [models.CollectionIssueSeverity16](../models/collectionissueseverity16.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `source`                                                                   | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |