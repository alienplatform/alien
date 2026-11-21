# AiAvailabilityObservationQuotaWarning

One quota the account has set well below the cloud's default (under 10%).

## Example Usage

```typescript
import { AiAvailabilityObservationQuotaWarning } from "@alienplatform/platform-api/models";

let value: AiAvailabilityObservationQuotaWarning = {
  applied: 8587.81,
  default: 3734.9,
  quotaName: "<value>",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `applied`          | *number*           | :heavy_check_mark: | N/A                |
| `default`          | *number*           | :heavy_check_mark: | N/A                |
| `quotaName`        | *string*           | :heavy_check_mark: | N/A                |