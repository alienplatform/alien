# TargetDeploymentPasswordSecretArn

## Example Usage

```typescript
import { TargetDeploymentPasswordSecretArn } from "@alienplatform/platform-api/models";

let value: TargetDeploymentPasswordSecretArn = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                  | [models.TargetDeploymentPasswordSecretArnSecretRef](../models/targetdeploymentpasswordsecretarnsecretref.md) | :heavy_check_mark:                                                                                           | Reference to a Kubernetes Secret                                                                             |