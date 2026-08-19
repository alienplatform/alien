# TargetDeploymentRepositoryPrefix1

## Example Usage

```typescript
import { TargetDeploymentRepositoryPrefix1 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentRepositoryPrefix1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                  | [models.TargetDeploymentRepositoryPrefixSecretRef1](../models/targetdeploymentrepositoryprefixsecretref1.md) | :heavy_check_mark:                                                                                           | Reference to a Kubernetes Secret                                                                             |