# Pricing

## Example Usage

```typescript
import { Pricing } from "@alienplatform/platform-api/models/operations";

let value: Pricing = {
  label: "Estimated provider cost",
  coverage: 8922.19,
  revision: "<value>",
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `label`                                              | [operations.Label](../../models/operations/label.md) | :heavy_check_mark:                                   | N/A                                                  |
| `coverage`                                           | *number*                                             | :heavy_check_mark:                                   | N/A                                                  |
| `revision`                                           | *string*                                             | :heavy_check_mark:                                   | N/A                                                  |