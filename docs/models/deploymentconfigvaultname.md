# DeploymentConfigVaultName

## Example Usage

```typescript
import { DeploymentConfigVaultName } from "@alienplatform/platform-api/models";

let value: DeploymentConfigVaultName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                  | [models.DeploymentConfigVaultNameSecretRef](../models/deploymentconfigvaultnamesecretref.md) | :heavy_check_mark:                                                                           | Reference to a Kubernetes Secret                                                             |