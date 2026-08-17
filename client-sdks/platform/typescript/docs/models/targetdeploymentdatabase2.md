# TargetDeploymentDatabase2

## Example Usage

```typescript
import { TargetDeploymentDatabase2 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentDatabase2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                  | [models.TargetDeploymentDatabaseSecretRef2](../models/targetdeploymentdatabasesecretref2.md) | :heavy_check_mark:                                                                           | Reference to a Kubernetes Secret                                                             |