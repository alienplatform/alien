# TargetDeploymentUsername5

## Example Usage

```typescript
import { TargetDeploymentUsername5 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentUsername5 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                  | [models.TargetDeploymentUsernameSecretRef5](../models/targetdeploymentusernamesecretref5.md) | :heavy_check_mark:                                                                           | Reference to a Kubernetes Secret                                                             |