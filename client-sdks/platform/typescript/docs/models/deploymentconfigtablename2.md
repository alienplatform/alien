# DeploymentConfigTableName2

## Example Usage

```typescript
import { DeploymentConfigTableName2 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigTableName2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                    | [models.DeploymentConfigTableNameSecretRef2](../models/deploymentconfigtablenamesecretref2.md) | :heavy_check_mark:                                                                             | Reference to a Kubernetes Secret                                                               |