# DeploymentConfigPasswordSecretName

## Example Usage

```typescript
import { DeploymentConfigPasswordSecretName } from "@alienplatform/platform-api/models";

let value: DeploymentConfigPasswordSecretName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                    | [models.DeploymentConfigPasswordSecretNameSecretRef](../models/deploymentconfigpasswordsecretnamesecretref.md) | :heavy_check_mark:                                                                                             | Reference to a Kubernetes Secret                                                                               |