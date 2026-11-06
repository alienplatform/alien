# HistoryAvailable

## Example Usage

```typescript
import { HistoryAvailable } from "@alienplatform/platform-api/models";

let value: HistoryAvailable = {
  status: "available",
  bucketSeconds: 7326.53,
  buckets: [],
};
```

## Fields

| Field                                  | Type                                   | Required                               | Description                            |
| -------------------------------------- | -------------------------------------- | -------------------------------------- | -------------------------------------- |
| `status`                               | *"available"*                          | :heavy_check_mark:                     | N/A                                    |
| `bucketSeconds`                        | *number*                               | :heavy_check_mark:                     | N/A                                    |
| `buckets`                              | [models.Bucket](../models/bucket.md)[] | :heavy_check_mark:                     | N/A                                    |