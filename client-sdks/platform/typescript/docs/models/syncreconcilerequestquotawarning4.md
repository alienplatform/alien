# SyncReconcileRequestQuotaWarning4

One quota the account has set well below the cloud's default (under 10%).

## Example Usage

```typescript
import { SyncReconcileRequestQuotaWarning4 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestQuotaWarning4 = {
  applied: 82.19,
  default: 2033.42,
  quotaName: "<value>",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `applied`          | *number*           | :heavy_check_mark: | N/A                |
| `default`          | *number*           | :heavy_check_mark: | N/A                |
| `quotaName`        | *string*           | :heavy_check_mark: | N/A                |