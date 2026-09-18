# TargetDeploymentDatabase6

## Example Usage

```typescript
import { TargetDeploymentDatabase6 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentDatabase6 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                  | [models.TargetDeploymentDatabaseSecretRef6](../models/targetdeploymentdatabasesecretref6.md) | :heavy_check_mark:                                                                           | Reference to a Kubernetes Secret                                                             |