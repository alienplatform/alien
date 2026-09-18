# TargetDeploymentPasswordSecretUri

## Example Usage

```typescript
import { TargetDeploymentPasswordSecretUri } from "@alienplatform/platform-api/models";

let value: TargetDeploymentPasswordSecretUri = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                  | [models.TargetDeploymentPasswordSecretUriSecretRef](../models/targetdeploymentpasswordsecreturisecretref.md) | :heavy_check_mark:                                                                                           | Reference to a Kubernetes Secret                                                                             |