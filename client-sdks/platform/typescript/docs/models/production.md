# Production

## Example Usage

```typescript
import { Production } from "@alienplatform/platform-api/models";

let value: Production = {
  releaseId: "<id>",
  deploymentCount: 5425.6,
  currentReleaseCount: 8087.52,
  pinnedCount: 9594.41,
  updatingCount: 9731.42,
  failedCount: 6693.52,
  changedAt: new Date("2025-05-10T16:23:01.835Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `releaseId`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `deploymentCount`                                                                             | *number*                                                                                      | :heavy_check_mark:                                                                            | Application deployments following production                                                  |
| `currentReleaseCount`                                                                         | *number*                                                                                      | :heavy_check_mark:                                                                            | Of those that aren't pinned, how many run production's current release                        |
| `pinnedCount`                                                                                 | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `updatingCount`                                                                               | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `failedCount`                                                                                 | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `changedAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | When production last moved to a release                                                       |