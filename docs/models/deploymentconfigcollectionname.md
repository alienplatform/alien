# DeploymentConfigCollectionName

## Example Usage

```typescript
import { DeploymentConfigCollectionName } from "@alienplatform/platform-api/models";

let value: DeploymentConfigCollectionName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                            | [models.DeploymentConfigCollectionNameSecretRef](../models/deploymentconfigcollectionnamesecretref.md) | :heavy_check_mark:                                                                                     | Reference to a Kubernetes Secret                                                                       |