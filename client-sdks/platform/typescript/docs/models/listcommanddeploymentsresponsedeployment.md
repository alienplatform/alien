# ListCommandDeploymentsResponseDeployment

## Example Usage

```typescript
import { ListCommandDeploymentsResponseDeployment } from "@alienplatform/platform-api/models";

let value: ListCommandDeploymentsResponseDeployment = {
  id: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  name: "<value>",
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        | Example                                                                                                            |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `id`                                                                                                               | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | Unique identifier for the deployment.                                                                              | dep_0c29fq4a2yjb7kx3smwdgxlc                                                                                       |
| `name`                                                                                                             | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |                                                                                                                    |
| `deploymentGroup`                                                                                                  | [models.ListCommandDeploymentsResponseDeploymentGroup](../models/listcommanddeploymentsresponsedeploymentgroup.md) | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |                                                                                                                    |