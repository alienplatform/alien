# TargetDeploymentVaultName

## Example Usage

```typescript
import { TargetDeploymentVaultName } from "@alienplatform/platform-api/models";

let value: TargetDeploymentVaultName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                  | [models.TargetDeploymentVaultNameSecretRef](../models/targetdeploymentvaultnamesecretref.md) | :heavy_check_mark:                                                                           | Reference to a Kubernetes Secret                                                             |