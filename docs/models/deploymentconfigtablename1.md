# DeploymentConfigTableName1

## Example Usage

```typescript
import { DeploymentConfigTableName1 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigTableName1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                    | [models.DeploymentConfigTableNameSecretRef1](../models/deploymentconfigtablenamesecretref1.md) | :heavy_check_mark:                                                                             | Reference to a Kubernetes Secret                                                               |