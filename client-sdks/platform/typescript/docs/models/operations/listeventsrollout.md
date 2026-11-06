# ListEventsRollout

## Example Usage

```typescript
import { ListEventsRollout } from "@alienplatform/platform-api/models/operations";

let value: ListEventsRollout = {
  releaseId: "<id>",
  day: "<value>",
  deployments: 995310,
};
```

## Fields

| Field                                             | Type                                              | Required                                          | Description                                       |
| ------------------------------------------------- | ------------------------------------------------- | ------------------------------------------------- | ------------------------------------------------- |
| `releaseId`                                       | *string*                                          | :heavy_check_mark:                                | N/A                                               |
| `day`                                             | *string*                                          | :heavy_check_mark:                                | UTC day, YYYY-MM-DD                               |
| `deployments`                                     | *number*                                          | :heavy_check_mark:                                | Distinct deployments the release reached that day |