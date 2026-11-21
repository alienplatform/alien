# QuotaWarning1

One quota the account has set well below the cloud's default (under 10%).

## Example Usage

```typescript
import { QuotaWarning1 } from "@alienplatform/platform-api/models/operations";

let value: QuotaWarning1 = {
  applied: 3381.46,
  default: 4803.34,
  quotaName: "<value>",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `applied`          | *number*           | :heavy_check_mark: | N/A                |
| `default`          | *number*           | :heavy_check_mark: | N/A                |
| `quotaName`        | *string*           | :heavy_check_mark: | N/A                |