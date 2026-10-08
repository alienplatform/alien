# GetDeploymentTargetRequest

## Example Usage

```typescript
import { GetDeploymentTargetRequest } from "@alienplatform/manager-api/models/operations";

let value: GetDeploymentTargetRequest = {
  id: "<id>",
};
```

## Fields

| Field                                                           | Type                                                            | Required                                                        | Description                                                     |
| --------------------------------------------------------------- | --------------------------------------------------------------- | --------------------------------------------------------------- | --------------------------------------------------------------- |
| `id`                                                            | *string*                                                        | :heavy_check_mark:                                              | Deployment ID                                                   |
| `releaseId`                                                     | *string*                                                        | :heavy_minus_sign:                                              | Release to target; defaults to the deployment's pin or channel. |