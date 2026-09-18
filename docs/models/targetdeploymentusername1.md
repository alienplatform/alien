# TargetDeploymentUsername1

## Example Usage

```typescript
import { TargetDeploymentUsername1 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentUsername1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                  | [models.TargetDeploymentUsernameSecretRef1](../models/targetdeploymentusernamesecretref1.md) | :heavy_check_mark:                                                                           | Reference to a Kubernetes Secret                                                             |