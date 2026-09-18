# DeploymentConfigRepositoryName

## Example Usage

```typescript
import { DeploymentConfigRepositoryName } from "@alienplatform/platform-api/models";

let value: DeploymentConfigRepositoryName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                            | [models.DeploymentConfigRepositoryNameSecretRef](../models/deploymentconfigrepositorynamesecretref.md) | :heavy_check_mark:                                                                                     | Reference to a Kubernetes Secret                                                                       |