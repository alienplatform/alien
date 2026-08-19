# TargetDeploymentPasswordSecretName

## Example Usage

```typescript
import { TargetDeploymentPasswordSecretName } from "@alienplatform/platform-api/models";

let value: TargetDeploymentPasswordSecretName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                    | [models.TargetDeploymentPasswordSecretNameSecretRef](../models/targetdeploymentpasswordsecretnamesecretref.md) | :heavy_check_mark:                                                                                             | Reference to a Kubernetes Secret                                                                               |