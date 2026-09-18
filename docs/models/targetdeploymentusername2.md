# TargetDeploymentUsername2

## Example Usage

```typescript
import { TargetDeploymentUsername2 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentUsername2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                  | [models.TargetDeploymentUsernameSecretRef2](../models/targetdeploymentusernamesecretref2.md) | :heavy_check_mark:                                                                           | Reference to a Kubernetes Secret                                                             |