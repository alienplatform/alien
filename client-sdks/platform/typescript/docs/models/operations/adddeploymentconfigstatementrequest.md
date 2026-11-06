# AddDeploymentConfigStatementRequest

## Example Usage

```typescript
import { AddDeploymentConfigStatementRequest } from "@alienplatform/platform-api/models/operations";

let value: AddDeploymentConfigStatementRequest = {
  id: "dep_0c29fq4a2yjb7kx3smwdgxlcxd7f",
};
```

## Fields

| Field                                                                   | Type                                                                    | Required                                                                | Description                                                             | Example                                                                 |
| ----------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| `id`                                                                    | *string*                                                                | :heavy_check_mark:                                                      | Unique identifier for the deployment.                                   | dep_0c29fq4a2yjb7kx3smwdgxlcxd7f                                        |
| `signedStatementRequest`                                                | [models.SignedStatementRequest](../../models/signedstatementrequest.md) | :heavy_minus_sign:                                                      | N/A                                                                     |                                                                         |