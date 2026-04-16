# CommandStatusResponse

Response to status queries

## Example Usage

```typescript
import { CommandStatusResponse } from "@alienplatform/manager-api/models";

let value: CommandStatusResponse = {
  attempt: 289238,
  commandId: "<id>",
  state: "DISPATCHED",
};
```

## Fields

| Field                                             | Type                                              | Required                                          | Description                                       |
| ------------------------------------------------- | ------------------------------------------------- | ------------------------------------------------- | ------------------------------------------------- |
| `attempt`                                         | *number*                                          | :heavy_check_mark:                                | Current attempt number                            |
| `commandId`                                       | *string*                                          | :heavy_check_mark:                                | Command identifier                                |
| `response`                                        | *models.CommandResponse*                          | :heavy_minus_sign:                                | N/A                                               |
| `state`                                           | [models.CommandState](../models/commandstate.md)  | :heavy_check_mark:                                | Command states in the Commands protocol lifecycle |