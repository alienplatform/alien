# TargetDeploymentUsername4

## Example Usage

```typescript
import { TargetDeploymentUsername4 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentUsername4 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                  | [models.TargetDeploymentUsernameSecretRef4](../models/targetdeploymentusernamesecretref4.md) | :heavy_check_mark:                                                                           | Reference to a Kubernetes Secret                                                             |