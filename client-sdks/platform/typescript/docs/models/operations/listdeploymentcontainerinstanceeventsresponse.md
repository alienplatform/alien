# ListDeploymentContainerInstanceEventsResponse

Container orchestration events.

## Example Usage

```typescript
import { ListDeploymentContainerInstanceEventsResponse } from "@aliendotdev/platform-api/models/operations";

let value: ListDeploymentContainerInstanceEventsResponse = {
  events: [
    {
      eventId: "<id>",
      type: "warning",
      reason: "<value>",
      involvedObject: {
        type: "<value>",
        id: "<id>",
      },
      firstTimestamp: "<value>",
      lastTimestamp: "<value>",
      count: 956857,
    },
  ],
};
```

## Fields

| Field                                                                                                                            | Type                                                                                                                             | Required                                                                                                                         | Description                                                                                                                      |
| -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `events`                                                                                                                         | [operations.ListDeploymentContainerInstanceEventsEvent](../../models/operations/listdeploymentcontainerinstanceeventsevent.md)[] | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |