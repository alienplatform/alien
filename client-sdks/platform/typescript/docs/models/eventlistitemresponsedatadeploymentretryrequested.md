# EventListItemResponseDataDeploymentRetryRequested

## Example Usage

```typescript
import { EventListItemResponseDataDeploymentRetryRequested } from "@alienplatform/platform-api/models";

let value: EventListItemResponseDataDeploymentRetryRequested = {
  deploymentId: "<id>",
  type: "DeploymentRetryRequested",
};
```

## Fields

| Field                                                             | Type                                                              | Required                                                          | Description                                                       |
| ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- |
| `actor`                                                           | *models.EventListItemResponseActorUnion1*                         | :heavy_minus_sign:                                                | N/A                                                               |
| `attemptedReleaseId`                                              | *string*                                                          | :heavy_minus_sign:                                                | ID of the release that the failed attempt was targeting, if known |
| `deploymentId`                                                    | *string*                                                          | :heavy_check_mark:                                                | ID of the deployment                                              |
| `previousError`                                                   | *models.EventListItemResponsePreviousErrorUnion*                  | :heavy_minus_sign:                                                | N/A                                                               |
| `type`                                                            | *"DeploymentRetryRequested"*                                      | :heavy_check_mark:                                                | N/A                                                               |
