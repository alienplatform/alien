# TargetDeploymentDatabase4

## Example Usage

```typescript
import { TargetDeploymentDatabase4 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentDatabase4 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                  | [models.TargetDeploymentDatabaseSecretRef4](../models/targetdeploymentdatabasesecretref4.md) | :heavy_check_mark:                                                                           | Reference to a Kubernetes Secret                                                             |