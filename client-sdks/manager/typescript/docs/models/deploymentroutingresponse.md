# DeploymentRoutingResponse

What a deployment follows and the release that makes it run.

## Example Usage

```typescript
import { DeploymentRoutingResponse } from "@alienplatform/manager-api/models";

let value: DeploymentRoutingResponse = {
  channel: "<value>",
  deploymentId: "<id>",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `channel`                                                                | *string*                                                                 | :heavy_check_mark:                                                       | N/A                                                                      |
| `deploymentId`                                                           | *string*                                                                 | :heavy_check_mark:                                                       | N/A                                                                      |
| `pinnedReleaseId`                                                        | *string*                                                                 | :heavy_minus_sign:                                                       | N/A                                                                      |
| `releaseId`                                                              | *string*                                                                 | :heavy_minus_sign:                                                       | The release the deployment is sent: the pin, else the channel's release. |