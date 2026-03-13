# ListDeploymentContainerEventsResponse

List of orchestration events.

## Example Usage

```typescript
import { ListDeploymentContainerEventsResponse } from "@alienplatform/platform-api/models/operations";

let value: ListDeploymentContainerEventsResponse = {
  events: [
    {
      eventId: "<id>",
      type: "info",
      reason: "<value>",
      involvedObject: {
        type: "<value>",
        id: "<id>",
      },
      firstTimestamp: "<value>",
      lastTimestamp: "<value>",
      count: 767479,
    },
  ],
};
```

## Fields

| Field                                                                                                            | Type                                                                                                             | Required                                                                                                         | Description                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `events`                                                                                                         | [operations.ListDeploymentContainerEventsEvent](../../models/operations/listdeploymentcontainereventsevent.md)[] | :heavy_check_mark:                                                                                               | N/A                                                                                                              |