# TargetDeploymentApiKey

## Example Usage

```typescript
import { TargetDeploymentApiKey } from "@alienplatform/platform-api/models";

let value: TargetDeploymentApiKey = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `secretRef`                                                                            | [models.TargetDeploymentApiKeySecretRef](../models/targetdeploymentapikeysecretref.md) | :heavy_check_mark:                                                                     | Reference to a Kubernetes Secret                                                       |