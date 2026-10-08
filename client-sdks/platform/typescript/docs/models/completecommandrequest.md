# CompleteCommandRequest

## Example Usage

```typescript
import { CompleteCommandRequest } from "@alienplatform/platform-api/models";

let value: CompleteCommandRequest = {
  state: "EXPIRED",
  completedAt: new Date("2025-07-20T12:51:50.395Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `state`                                                                                       | [models.CompleteCommandRequestState](../models/completecommandrequeststate.md)                | :heavy_check_mark:                                                                            | Terminal state to transition to                                                               |
| `completedAt`                                                                                 | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | When the command completed                                                                    |
| `responseSizeBytes`                                                                           | *number*                                                                                      | :heavy_minus_sign:                                                                            | Size of response in bytes                                                                     |
| `error`                                                                                       | Record<string, *any*>                                                                         | :heavy_minus_sign:                                                                            | Error details if failed                                                                       |