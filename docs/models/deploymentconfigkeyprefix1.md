# DeploymentConfigKeyPrefix1

## Example Usage

```typescript
import { DeploymentConfigKeyPrefix1 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigKeyPrefix1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                    | [models.DeploymentConfigKeyPrefixSecretRef1](../models/deploymentconfigkeyprefixsecretref1.md) | :heavy_check_mark:                                                                             | Reference to a Kubernetes Secret                                                               |