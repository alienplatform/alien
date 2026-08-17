# DeploymentConfigDatabase6

## Example Usage

```typescript
import { DeploymentConfigDatabase6 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigDatabase6 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                  | [models.DeploymentConfigDatabaseSecretRef6](../models/deploymentconfigdatabasesecretref6.md) | :heavy_check_mark:                                                                           | Reference to a Kubernetes Secret                                                             |