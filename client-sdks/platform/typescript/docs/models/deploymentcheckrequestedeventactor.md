# DeploymentCheckRequestedEventActor

Authenticated principal that requested a deployment intent event.

## Example Usage

```typescript
import { DeploymentCheckRequestedEventActor } from "@alienplatform/platform-api/models";

let value: DeploymentCheckRequestedEventActor = {
  id: "<id>",
  kind: "user",
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `email`                                                                                    | *string*                                                                                   | :heavy_minus_sign:                                                                         | User email when the principal is a user.                                                   |
| `id`                                                                                       | *string*                                                                                   | :heavy_check_mark:                                                                         | Stable user or service-account identifier.                                                 |
| `kind`                                                                                     | [models.DeploymentCheckRequestedEventKind](../models/deploymentcheckrequestedeventkind.md) | :heavy_check_mark:                                                                         | Type of authenticated principal that requested an event.                                   |
| `via`                                                                                      | *models.DeploymentCheckRequestedEventViaUnion*                                             | :heavy_minus_sign:                                                                         | N/A                                                                                        |