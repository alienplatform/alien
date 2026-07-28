# ReleaseRolloutStateCounts

Deployment counts per rollout state across all deployments the filters allow (ignores the state filter)

## Example Usage

```typescript
import { ReleaseRolloutStateCounts } from "@alienplatform/platform-api/models";

let value: ReleaseRolloutStateCounts = {
  updated: 296286,
  updating: 209667,
  failed: 197301,
  pending: 996144,
  pinnedOther: 119808,
  superseded: 913884,
  onOther: 305959,
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `updated`          | *number*           | :heavy_check_mark: | N/A                |
| `updating`         | *number*           | :heavy_check_mark: | N/A                |
| `failed`           | *number*           | :heavy_check_mark: | N/A                |
| `pending`          | *number*           | :heavy_check_mark: | N/A                |
| `pinnedOther`      | *number*           | :heavy_check_mark: | N/A                |
| `superseded`       | *number*           | :heavy_check_mark: | N/A                |
| `onOther`          | *number*           | :heavy_check_mark: | N/A                |
