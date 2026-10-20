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
  waitingOnOwner: 119808,
  waitingOnInputs: 913884,
  pinnedOther: 305959,
  superseded: 712862,
  onOther: 173673,
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `updated`          | *number*           | :heavy_check_mark: | N/A                |
| `updating`         | *number*           | :heavy_check_mark: | N/A                |
| `failed`           | *number*           | :heavy_check_mark: | N/A                |
| `pending`          | *number*           | :heavy_check_mark: | N/A                |
| `waitingOnOwner`   | *number*           | :heavy_check_mark: | N/A                |
| `waitingOnInputs`  | *number*           | :heavy_check_mark: | N/A                |
| `pinnedOther`      | *number*           | :heavy_check_mark: | N/A                |
| `superseded`       | *number*           | :heavy_check_mark: | N/A                |
| `onOther`          | *number*           | :heavy_check_mark: | N/A                |