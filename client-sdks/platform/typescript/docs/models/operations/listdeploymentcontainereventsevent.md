# ListDeploymentContainerEventsEvent

## Example Usage

```typescript
import { ListDeploymentContainerEventsEvent } from "@alienplatform/platform-api/models/operations";

let value: ListDeploymentContainerEventsEvent = {
  eventId: "<id>",
  type: "info",
  reason: "<value>",
  involvedObject: {
    type: "<value>",
    id: "<id>",
  },
  firstTimestamp: "<value>",
  lastTimestamp: "<value>",
  count: 562319,
};
```

## Fields

| Field                                                                                                                            | Type                                                                                                                             | Required                                                                                                                         | Description                                                                                                                      |
| -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `eventId`                                                                                                                        | *string*                                                                                                                         | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |
| `type`                                                                                                                           | [operations.ListDeploymentContainerEventsEventType](../../models/operations/listdeploymentcontainereventseventtype.md)           | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |
| `reason`                                                                                                                         | *string*                                                                                                                         | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |
| `message`                                                                                                                        | *string*                                                                                                                         | :heavy_minus_sign:                                                                                                               | N/A                                                                                                                              |
| `involvedObject`                                                                                                                 | [operations.ListDeploymentContainerEventsInvolvedObject](../../models/operations/listdeploymentcontainereventsinvolvedobject.md) | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |
| `firstTimestamp`                                                                                                                 | *string*                                                                                                                         | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |
| `lastTimestamp`                                                                                                                  | *string*                                                                                                                         | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |
| `count`                                                                                                                          | *number*                                                                                                                         | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |
| `additionalProperties`                                                                                                           | Record<string, *any*>                                                                                                            | :heavy_minus_sign:                                                                                                               | N/A                                                                                                                              |