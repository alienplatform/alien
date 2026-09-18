# DeploymentConfigPasswordSecretUri

## Example Usage

```typescript
import { DeploymentConfigPasswordSecretUri } from "@alienplatform/platform-api/models";

let value: DeploymentConfigPasswordSecretUri = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                  | [models.DeploymentConfigPasswordSecretUriSecretRef](../models/deploymentconfigpasswordsecreturisecretref.md) | :heavy_check_mark:                                                                                           | Reference to a Kubernetes Secret                                                                             |