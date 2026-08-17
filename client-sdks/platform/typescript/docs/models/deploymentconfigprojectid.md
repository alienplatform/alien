# DeploymentConfigProjectId

## Example Usage

```typescript
import { DeploymentConfigProjectId } from "@alienplatform/platform-api/models";

let value: DeploymentConfigProjectId = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                  | [models.DeploymentConfigProjectIdSecretRef](../models/deploymentconfigprojectidsecretref.md) | :heavy_check_mark:                                                                           | Reference to a Kubernetes Secret                                                             |