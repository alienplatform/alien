# DeploymentConfigDatabaseId

## Example Usage

```typescript
import { DeploymentConfigDatabaseId } from "@alienplatform/platform-api/models";

let value: DeploymentConfigDatabaseId = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                    | [models.DeploymentConfigDatabaseIdSecretRef](../models/deploymentconfigdatabaseidsecretref.md) | :heavy_check_mark:                                                                             | Reference to a Kubernetes Secret                                                               |