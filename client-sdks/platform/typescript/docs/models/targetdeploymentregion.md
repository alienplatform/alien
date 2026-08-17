# TargetDeploymentRegion

## Example Usage

```typescript
import { TargetDeploymentRegion } from "@alienplatform/platform-api/models";

let value: TargetDeploymentRegion = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `secretRef`                                                                            | [models.TargetDeploymentRegionSecretRef](../models/targetdeploymentregionsecretref.md) | :heavy_check_mark:                                                                     | Reference to a Kubernetes Secret                                                       |