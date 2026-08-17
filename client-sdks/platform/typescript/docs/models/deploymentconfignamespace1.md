# DeploymentConfigNamespace1

## Example Usage

```typescript
import { DeploymentConfigNamespace1 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigNamespace1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                    | [models.DeploymentConfigNamespaceSecretRef1](../models/deploymentconfignamespacesecretref1.md) | :heavy_check_mark:                                                                             | Reference to a Kubernetes Secret                                                               |