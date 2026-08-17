# DeploymentConfigRepositoryPrefix1

## Example Usage

```typescript
import { DeploymentConfigRepositoryPrefix1 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigRepositoryPrefix1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                  | [models.DeploymentConfigRepositoryPrefixSecretRef1](../models/deploymentconfigrepositoryprefixsecretref1.md) | :heavy_check_mark:                                                                                           | Reference to a Kubernetes Secret                                                                             |