# RecentLogsResponse

## Example Usage

```typescript
import { RecentLogsResponse } from "@alienplatform/manager-api/models";

let value: RecentLogsResponse = {
  items: [
    {
      attributes: {
        "key": "<value>",
      },
      message: "<value>",
      severity: "<value>",
      timestamp: new Date("2025-02-10T09:34:37.450Z"),
    },
  ],
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `items`                                                | [models.RecentLogEntry](../models/recentlogentry.md)[] | :heavy_check_mark:                                     | Oldest first.                                          |