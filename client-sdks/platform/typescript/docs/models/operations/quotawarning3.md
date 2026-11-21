# QuotaWarning3

One quota the account has set well below the cloud's default (under 10%).

## Example Usage

```typescript
import { QuotaWarning3 } from "@alienplatform/platform-api/models/operations";

let value: QuotaWarning3 = {
  applied: 1002.1,
  default: 1102.46,
  quotaName: "<value>",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `applied`          | *number*           | :heavy_check_mark: | N/A                |
| `default`          | *number*           | :heavy_check_mark: | N/A                |
| `quotaName`        | *string*           | :heavy_check_mark: | N/A                |