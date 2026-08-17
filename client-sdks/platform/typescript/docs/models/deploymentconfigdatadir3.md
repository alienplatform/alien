# DeploymentConfigDataDir3

## Example Usage

```typescript
import { DeploymentConfigDataDir3 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigDataDir3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                | [models.DeploymentConfigDataDirSecretRef3](../models/deploymentconfigdatadirsecretref3.md) | :heavy_check_mark:                                                                         | Reference to a Kubernetes Secret                                                           |