# DeploymentConfigDatabase2

## Example Usage

```typescript
import { DeploymentConfigDatabase2 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigDatabase2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                  | [models.DeploymentConfigDatabaseSecretRef2](../models/deploymentconfigdatabasesecretref2.md) | :heavy_check_mark:                                                                           | Reference to a Kubernetes Secret                                                             |