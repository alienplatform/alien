# SetDeploymentChannelRequest

## Example Usage

```typescript
import { SetDeploymentChannelRequest } from "@alienplatform/manager-api/models/operations";

let value: SetDeploymentChannelRequest = {
  id: "<id>",
  setDeploymentChannelRequest: {
    channel: "<value>",
  },
};
```

## Fields

| Field                                                                             | Type                                                                              | Required                                                                          | Description                                                                       |
| --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| `id`                                                                              | *string*                                                                          | :heavy_check_mark:                                                                | Deployment ID                                                                     |
| `setDeploymentChannelRequest`                                                     | [models.SetDeploymentChannelRequest](../../models/setdeploymentchannelrequest.md) | :heavy_check_mark:                                                                | N/A                                                                               |