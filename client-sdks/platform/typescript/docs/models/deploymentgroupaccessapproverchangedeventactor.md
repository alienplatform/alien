# DeploymentGroupAccessApproverChangedEventActor

Authenticated principal that requested a deployment intent event.

## Example Usage

```typescript
import { DeploymentGroupAccessApproverChangedEventActor } from "@alienplatform/platform-api/models";

let value: DeploymentGroupAccessApproverChangedEventActor = {
  id: "<id>",
  kind: "serviceAccount",
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `email`                                                                                                            | *string*                                                                                                           | :heavy_minus_sign:                                                                                                 | User email when the principal is a user.                                                                           |
| `id`                                                                                                               | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | Stable user or service-account identifier.                                                                         |
| `kind`                                                                                                             | [models.DeploymentGroupAccessApproverChangedEventKind](../models/deploymentgroupaccessapproverchangedeventkind.md) | :heavy_check_mark:                                                                                                 | Type of authenticated principal that requested an event.                                                           |
| `via`                                                                                                              | *models.DeploymentGroupAccessApproverChangedEventViaUnion*                                                         | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |