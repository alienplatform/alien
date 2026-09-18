# DeploymentConfigDatabase1

## Example Usage

```typescript
import { DeploymentConfigDatabase1 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigDatabase1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                  | [models.DeploymentConfigDatabaseSecretRef1](../models/deploymentconfigdatabasesecretref1.md) | :heavy_check_mark:                                                                           | Reference to a Kubernetes Secret                                                             |