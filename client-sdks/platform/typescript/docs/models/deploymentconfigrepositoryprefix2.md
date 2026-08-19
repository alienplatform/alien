# DeploymentConfigRepositoryPrefix2

## Example Usage

```typescript
import { DeploymentConfigRepositoryPrefix2 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigRepositoryPrefix2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                  | [models.DeploymentConfigRepositoryPrefixSecretRef2](../models/deploymentconfigrepositoryprefixsecretref2.md) | :heavy_check_mark:                                                                                           | Reference to a Kubernetes Secret                                                                             |