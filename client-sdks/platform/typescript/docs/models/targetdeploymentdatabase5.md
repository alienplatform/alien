# TargetDeploymentDatabase5

## Example Usage

```typescript
import { TargetDeploymentDatabase5 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentDatabase5 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                  | [models.TargetDeploymentDatabaseSecretRef5](../models/targetdeploymentdatabasesecretref5.md) | :heavy_check_mark:                                                                           | Reference to a Kubernetes Secret                                                             |