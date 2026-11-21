# StateCounts

Deployments per rollout state relative to this release, counted like the release's deployments list

## Example Usage

```typescript
import { StateCounts } from "@alienplatform/platform-api/models";

let value: StateCounts = {
  updated: 108199,
  updating: 71879,
  failed: 467215,
  pending: 211742,
  waitingOnOwner: 597652,
  waitingOnInputs: 267987,
  pinnedOther: 480270,
  superseded: 654665,
  onOther: 300380,
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