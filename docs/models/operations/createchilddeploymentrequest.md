# CreateChildDeploymentRequest

## Example Usage

```typescript
import { CreateChildDeploymentRequest } from "@alienplatform/platform-api/models/operations";

let value: CreateChildDeploymentRequest = {
  id: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  idempotencyKey: "<value>",
};
```

## Fields

| Field                                                                               | Type                                                                                | Required                                                                            | Description                                                                         | Example                                                                             |
| ----------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| `id`                                                                                | *string*                                                                            | :heavy_check_mark:                                                                  | Unique identifier for the deployment.                                               | dep_0c29fq4a2yjb7kx3smwdgxlc                                                        |
| `idempotencyKey`                                                                    | *string*                                                                            | :heavy_check_mark:                                                                  | N/A                                                                                 |                                                                                     |
| `createChildDeploymentRequest`                                                      | [models.CreateChildDeploymentRequest](../../models/createchilddeploymentrequest.md) | :heavy_minus_sign:                                                                  | N/A                                                                                 |                                                                                     |