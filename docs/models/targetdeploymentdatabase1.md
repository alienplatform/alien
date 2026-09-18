# TargetDeploymentDatabase1

## Example Usage

```typescript
import { TargetDeploymentDatabase1 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentDatabase1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                  | [models.TargetDeploymentDatabaseSecretRef1](../models/targetdeploymentdatabasesecretref1.md) | :heavy_check_mark:                                                                           | Reference to a Kubernetes Secret                                                             |