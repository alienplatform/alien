# DeploymentConfigDataDir2

## Example Usage

```typescript
import { DeploymentConfigDataDir2 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigDataDir2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                | [models.DeploymentConfigDataDirSecretRef2](../models/deploymentconfigdatadirsecretref2.md) | :heavy_check_mark:                                                                         | Reference to a Kubernetes Secret                                                           |