# DeleteDeploymentRequest

## Example Usage

```typescript
import { DeleteDeploymentRequest } from "@alienplatform/manager-api/models/operations";

let value: DeleteDeploymentRequest = {
  id: "<id>",
  deleteDeploymentRequest: {
    mode: "clean",
  },
};
```

## Fields

| Field                                                                     | Type                                                                      | Required                                                                  | Description                                                               |
| ------------------------------------------------------------------------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| `id`                                                                      | *string*                                                                  | :heavy_check_mark:                                                        | Deployment ID                                                             |
| `deleteDeploymentRequest`                                                 | [models.DeleteDeploymentRequest](../../models/deletedeploymentrequest.md) | :heavy_check_mark:                                                        | N/A                                                                       |