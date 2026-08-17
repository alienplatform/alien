# DeploymentConfigDatabase4

## Example Usage

```typescript
import { DeploymentConfigDatabase4 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigDatabase4 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                  | [models.DeploymentConfigDatabaseSecretRef4](../models/deploymentconfigdatabasesecretref4.md) | :heavy_check_mark:                                                                           | Reference to a Kubernetes Secret                                                             |