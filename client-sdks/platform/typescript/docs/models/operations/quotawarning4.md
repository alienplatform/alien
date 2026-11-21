# QuotaWarning4

One quota the account has set well below the cloud's default (under 10%).

## Example Usage

```typescript
import { QuotaWarning4 } from "@alienplatform/platform-api/models/operations";

let value: QuotaWarning4 = {
  applied: 9152.77,
  default: 2917.27,
  quotaName: "<value>",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `applied`          | *number*           | :heavy_check_mark: | N/A                |
| `default`          | *number*           | :heavy_check_mark: | N/A                |
| `quotaName`        | *string*           | :heavy_check_mark: | N/A                |