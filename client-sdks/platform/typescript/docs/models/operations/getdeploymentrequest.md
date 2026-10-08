# GetDeploymentRequest

## Example Usage

```typescript
import { GetDeploymentRequest } from "@alienplatform/platform-api/models/operations";

let value: GetDeploymentRequest = {
  id: "dep_0c29fq4a2yjb7kx3smwdgxlc",
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          | Example                                                                              |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `id`                                                                                 | *string*                                                                             | :heavy_check_mark:                                                                   | Unique identifier for the deployment.                                                | dep_0c29fq4a2yjb7kx3smwdgxlc                                                         |
| `include`                                                                            | [operations.GetDeploymentInclude](../../models/operations/getdeploymentinclude.md)[] | :heavy_minus_sign:                                                                   | Optional fields to include: release, deploymentGroup, project                        |                                                                                      |