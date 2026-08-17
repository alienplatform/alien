# DeploymentConfigApiKey

## Example Usage

```typescript
import { DeploymentConfigApiKey } from "@alienplatform/platform-api/models";

let value: DeploymentConfigApiKey = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `secretRef`                                                                            | [models.DeploymentConfigApiKeySecretRef](../models/deploymentconfigapikeysecretref.md) | :heavy_check_mark:                                                                     | Reference to a Kubernetes Secret                                                       |