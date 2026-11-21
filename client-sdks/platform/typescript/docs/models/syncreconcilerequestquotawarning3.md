# SyncReconcileRequestQuotaWarning3

One quota the account has set well below the cloud's default (under 10%).

## Example Usage

```typescript
import { SyncReconcileRequestQuotaWarning3 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestQuotaWarning3 = {
  applied: 7247.6,
  default: 1016.56,
  quotaName: "<value>",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `applied`          | *number*           | :heavy_check_mark: | N/A                |
| `default`          | *number*           | :heavy_check_mark: | N/A                |
| `quotaName`        | *string*           | :heavy_check_mark: | N/A                |