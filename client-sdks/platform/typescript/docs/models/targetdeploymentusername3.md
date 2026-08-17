# TargetDeploymentUsername3

## Example Usage

```typescript
import { TargetDeploymentUsername3 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentUsername3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                  | [models.TargetDeploymentUsernameSecretRef3](../models/targetdeploymentusernamesecretref3.md) | :heavy_check_mark:                                                                           | Reference to a Kubernetes Secret                                                             |