# QuotaWarning2

One quota the account has set well below the cloud's default (under 10%).

## Example Usage

```typescript
import { QuotaWarning2 } from "@alienplatform/platform-api/models/operations";

let value: QuotaWarning2 = {
  applied: 257.58,
  default: 410.23,
  quotaName: "<value>",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `applied`          | *number*           | :heavy_check_mark: | N/A                |
| `default`          | *number*           | :heavy_check_mark: | N/A                |
| `quotaName`        | *string*           | :heavy_check_mark: | N/A                |