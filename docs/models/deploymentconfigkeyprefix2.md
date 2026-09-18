# DeploymentConfigKeyPrefix2

## Example Usage

```typescript
import { DeploymentConfigKeyPrefix2 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigKeyPrefix2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                    | [models.DeploymentConfigKeyPrefixSecretRef2](../models/deploymentconfigkeyprefixsecretref2.md) | :heavy_check_mark:                                                                             | Reference to a Kubernetes Secret                                                               |