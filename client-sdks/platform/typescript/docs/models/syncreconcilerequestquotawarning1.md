# SyncReconcileRequestQuotaWarning1

One quota the account has set well below the cloud's default (under 10%).

## Example Usage

```typescript
import { SyncReconcileRequestQuotaWarning1 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestQuotaWarning1 = {
  applied: 2486.8,
  default: 8040.9,
  quotaName: "<value>",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `applied`          | *number*           | :heavy_check_mark: | N/A                |
| `default`          | *number*           | :heavy_check_mark: | N/A                |
| `quotaName`        | *string*           | :heavy_check_mark: | N/A                |