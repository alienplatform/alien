# SyncReconcileRequestQuotaWarning2

One quota the account has set well below the cloud's default (under 10%).

## Example Usage

```typescript
import { SyncReconcileRequestQuotaWarning2 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestQuotaWarning2 = {
  applied: 8476.45,
  default: 6764.85,
  quotaName: "<value>",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `applied`          | *number*           | :heavy_check_mark: | N/A                |
| `default`          | *number*           | :heavy_check_mark: | N/A                |
| `quotaName`        | *string*           | :heavy_check_mark: | N/A                |