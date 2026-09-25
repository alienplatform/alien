# EventListItemResponseActor7

Authenticated principal that requested a deployment intent event.

## Example Usage

```typescript
import { EventListItemResponseActor7 } from "@alienplatform/platform-api/models";

let value: EventListItemResponseActor7 = {
  id: "<id>",
  kind: "user",
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `email`                                                                      | *string*                                                                     | :heavy_minus_sign:                                                           | User email when the principal is a user.                                     |
| `id`                                                                         | *string*                                                                     | :heavy_check_mark:                                                           | Stable user or service-account identifier.                                   |
| `kind`                                                                       | [models.EventListItemResponseKind7](../models/eventlistitemresponsekind7.md) | :heavy_check_mark:                                                           | Type of authenticated principal that requested an event.                     |
| `via`                                                                        | *models.EventListItemResponseViaUnion7*                                      | :heavy_minus_sign:                                                           | N/A                                                                          |