# EventListItemResponseDataDeploymentReleaseChannelChanged

## Example Usage

```typescript
import { EventListItemResponseDataDeploymentReleaseChannelChanged } from "@alienplatform/platform-api/models";

let value: EventListItemResponseDataDeploymentReleaseChannelChanged = {
  channel: "<value>",
  deploymentId: "<id>",
  previousChannel: "<value>",
  type: "DeploymentReleaseChannelChanged",
};
```

## Fields

| Field                                     | Type                                      | Required                                  | Description                               |
| ----------------------------------------- | ----------------------------------------- | ----------------------------------------- | ----------------------------------------- |
| `actor`                                   | *models.EventListItemResponseActorUnion2* | :heavy_minus_sign:                        | N/A                                       |
| `channel`                                 | *string*                                  | :heavy_check_mark:                        | Newly followed channel                    |
| `deploymentId`                            | *string*                                  | :heavy_check_mark:                        | ID of the deployment                      |
| `previousChannel`                         | *string*                                  | :heavy_check_mark:                        | Previously followed channel               |
| `type`                                    | *"DeploymentReleaseChannelChanged"*       | :heavy_check_mark:                        | N/A                                       |