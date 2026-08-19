# TargetDeploymentCollectionName

## Example Usage

```typescript
import { TargetDeploymentCollectionName } from "@alienplatform/platform-api/models";

let value: TargetDeploymentCollectionName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                            | [models.TargetDeploymentCollectionNameSecretRef](../models/targetdeploymentcollectionnamesecretref.md) | :heavy_check_mark:                                                                                     | Reference to a Kubernetes Secret                                                                       |