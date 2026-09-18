# DeploymentConfigConnectionUrl

## Example Usage

```typescript
import { DeploymentConfigConnectionUrl } from "@alienplatform/platform-api/models";

let value: DeploymentConfigConnectionUrl = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                          | [models.DeploymentConfigConnectionUrlSecretRef](../models/deploymentconfigconnectionurlsecretref.md) | :heavy_check_mark:                                                                                   | Reference to a Kubernetes Secret                                                                     |