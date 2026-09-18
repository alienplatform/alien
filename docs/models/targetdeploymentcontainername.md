# TargetDeploymentContainerName

## Example Usage

```typescript
import { TargetDeploymentContainerName } from "@alienplatform/platform-api/models";

let value: TargetDeploymentContainerName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                          | [models.TargetDeploymentContainerNameSecretRef](../models/targetdeploymentcontainernamesecretref.md) | :heavy_check_mark:                                                                                   | Reference to a Kubernetes Secret                                                                     |