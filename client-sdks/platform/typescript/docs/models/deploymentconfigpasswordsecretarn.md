# DeploymentConfigPasswordSecretArn

## Example Usage

```typescript
import { DeploymentConfigPasswordSecretArn } from "@alienplatform/platform-api/models";

let value: DeploymentConfigPasswordSecretArn = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                  | [models.DeploymentConfigPasswordSecretArnSecretRef](../models/deploymentconfigpasswordsecretarnsecretref.md) | :heavy_check_mark:                                                                                           | Reference to a Kubernetes Secret                                                                             |