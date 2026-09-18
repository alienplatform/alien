# TargetDeploymentRepositoryPrefix2

## Example Usage

```typescript
import { TargetDeploymentRepositoryPrefix2 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentRepositoryPrefix2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                  | [models.TargetDeploymentRepositoryPrefixSecretRef2](../models/targetdeploymentrepositoryprefixsecretref2.md) | :heavy_check_mark:                                                                                           | Reference to a Kubernetes Secret                                                                             |