# UpdateCommandRequest

## Example Usage

```typescript
import { UpdateCommandRequest } from "@alienplatform/platform-api/models";

let value: UpdateCommandRequest = {};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `state`                                                                                       | [models.UpdateCommandRequestState](../models/updatecommandrequeststate.md)                    | :heavy_minus_sign:                                                                            | New command state                                                                             |
| `attempt`                                                                                     | *number*                                                                                      | :heavy_minus_sign:                                                                            | Current attempt number                                                                        |
| `dispatchedAt`                                                                                | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | When command was dispatched                                                                   |
| `completedAt`                                                                                 | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | When command completed                                                                        |
| `responseSizeBytes`                                                                           | *number*                                                                                      | :heavy_minus_sign:                                                                            | Size of response in bytes                                                                     |
| `error`                                                                                       | Record<string, *any*>                                                                         | :heavy_minus_sign:                                                                            | Error details if failed                                                                       |