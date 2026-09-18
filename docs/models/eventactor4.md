# EventActor4

Authenticated principal that requested a deployment intent event.

## Example Usage

```typescript
import { EventActor4 } from "@alienplatform/platform-api/models";

let value: EventActor4 = {
  id: "<id>",
  kind: "serviceAccount",
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `email`                                                  | *string*                                                 | :heavy_minus_sign:                                       | User email when the principal is a user.                 |
| `id`                                                     | *string*                                                 | :heavy_check_mark:                                       | Stable user or service-account identifier.               |
| `kind`                                                   | [models.EventKind4](../models/eventkind4.md)             | :heavy_check_mark:                                       | Type of authenticated principal that requested an event. |