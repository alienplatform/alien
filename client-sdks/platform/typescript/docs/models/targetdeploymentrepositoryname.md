# TargetDeploymentRepositoryName

## Example Usage

```typescript
import { TargetDeploymentRepositoryName } from "@alienplatform/platform-api/models";

let value: TargetDeploymentRepositoryName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                            | [models.TargetDeploymentRepositoryNameSecretRef](../models/targetdeploymentrepositorynamesecretref.md) | :heavy_check_mark:                                                                                     | Reference to a Kubernetes Secret                                                                       |