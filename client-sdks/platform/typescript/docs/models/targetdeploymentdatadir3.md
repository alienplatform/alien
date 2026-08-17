# TargetDeploymentDataDir3

## Example Usage

```typescript
import { TargetDeploymentDataDir3 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentDataDir3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                | [models.TargetDeploymentDataDirSecretRef3](../models/targetdeploymentdatadirsecretref3.md) | :heavy_check_mark:                                                                         | Reference to a Kubernetes Secret                                                           |