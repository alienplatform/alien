# DeploymentConfigDataDir1

## Example Usage

```typescript
import { DeploymentConfigDataDir1 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigDataDir1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                | [models.DeploymentConfigDataDirSecretRef1](../models/deploymentconfigdatadirsecretref1.md) | :heavy_check_mark:                                                                         | Reference to a Kubernetes Secret                                                           |