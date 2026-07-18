# Actor1

Authenticated principal that requested a deployment intent event.

## Example Usage

```typescript
import { Actor1 } from "@alienplatform/platform-api/models";

let value: Actor1 = {
  id: "<id>",
  kind: "user",
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `email`                                                  | *string*                                                 | :heavy_minus_sign:                                       | User email when the principal is a user.                 |
| `id`                                                     | *string*                                                 | :heavy_check_mark:                                       | Stable user or service-account identifier.               |
| `kind`                                                   | [models.EventKind1](../models/eventkind1.md)             | :heavy_check_mark:                                       | Type of authenticated principal that requested an event. |