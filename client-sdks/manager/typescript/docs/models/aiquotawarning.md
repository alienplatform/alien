# AiQuotaWarning

One quota the account has set well below the cloud's default (under 10%).

## Example Usage

```typescript
import { AiQuotaWarning } from "@alienplatform/manager-api/models";

let value: AiQuotaWarning = {
  applied: 4072.35,
  default: 4916.83,
  quotaName: "<value>",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `applied`          | *number*           | :heavy_check_mark: | N/A                |
| `default`          | *number*           | :heavy_check_mark: | N/A                |
| `quotaName`        | *string*           | :heavy_check_mark: | N/A                |
