# AgentSessionEventsResponse

## Example Usage

```typescript
import { AgentSessionEventsResponse } from "@alienplatform/platform-api/models";

let value: AgentSessionEventsResponse = {
  events: [],
  latestSeq: 8653.24,
  hasMore: false,
};
```

## Fields

| Field                        | Type                         | Required                     | Description                  |
| ---------------------------- | ---------------------------- | ---------------------------- | ---------------------------- |
| `events`                     | *models.AgentSessionEvent*[] | :heavy_check_mark:           | N/A                          |
| `latestSeq`                  | *number*                     | :heavy_check_mark:           | N/A                          |
| `hasMore`                    | *boolean*                    | :heavy_check_mark:           | N/A                          |
