# EventListItemResponseActor3

Authenticated principal that requested a deployment intent event.

## Example Usage

```typescript
import { EventListItemResponseActor3 } from "@alienplatform/platform-api/models";

let value: EventListItemResponseActor3 = {
  id: "<id>",
  kind: "serviceAccount",
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `email`                                                                      | *string*                                                                     | :heavy_minus_sign:                                                           | User email when the principal is a user.                                     |
| `id`                                                                         | *string*                                                                     | :heavy_check_mark:                                                           | Stable user or service-account identifier.                                   |
| `kind`                                                                       | [models.EventListItemResponseKind3](../models/eventlistitemresponsekind3.md) | :heavy_check_mark:                                                           | Type of authenticated principal that requested an event.                     |