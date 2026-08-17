# TargetDeploymentDataDir2

## Example Usage

```typescript
import { TargetDeploymentDataDir2 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentDataDir2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                | [models.TargetDeploymentDataDirSecretRef2](../models/targetdeploymentdatadirsecretref2.md) | :heavy_check_mark:                                                                         | Reference to a Kubernetes Secret                                                           |