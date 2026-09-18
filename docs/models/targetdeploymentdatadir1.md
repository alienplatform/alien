# TargetDeploymentDataDir1

## Example Usage

```typescript
import { TargetDeploymentDataDir1 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentDataDir1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                | [models.TargetDeploymentDataDirSecretRef1](../models/targetdeploymentdatadirsecretref1.md) | :heavy_check_mark:                                                                         | Reference to a Kubernetes Secret                                                           |