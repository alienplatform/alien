# DeploymentConfigNamespace2

## Example Usage

```typescript
import { DeploymentConfigNamespace2 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigNamespace2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                    | [models.DeploymentConfigNamespaceSecretRef2](../models/deploymentconfignamespacesecretref2.md) | :heavy_check_mark:                                                                             | Reference to a Kubernetes Secret                                                               |