# DeleteDeploymentRequest

## Example Usage

```typescript
import { DeleteDeploymentRequest } from "@alienplatform/manager-api/models/operations";

let value: DeleteDeploymentRequest = {
  id: "<id>",
};
```

## Fields

| Field                                                             | Type                                                              | Required                                                          | Description                                                       |
| ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- |
| `id`                                                              | *string*                                                          | :heavy_check_mark:                                                | Deployment ID                                                     |
| `force`                                                           | *boolean*                                                         | :heavy_minus_sign:                                                | Force delete without running cleanup (immediately removes record) |