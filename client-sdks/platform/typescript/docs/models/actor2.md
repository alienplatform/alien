# Actor2

Authenticated principal that requested a deployment intent event.

## Example Usage

```typescript
import { Actor2 } from "@alienplatform/platform-api/models";

let value: Actor2 = {
  id: "<id>",
  kind: "serviceAccount",
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `email`                                                  | *string*                                                 | :heavy_minus_sign:                                       | User email when the principal is a user.                 |
| `id`                                                     | *string*                                                 | :heavy_check_mark:                                       | Stable user or service-account identifier.               |
| `kind`                                                   | [models.EventKind2](../models/eventkind2.md)             | :heavy_check_mark:                                       | Type of authenticated principal that requested an event. |