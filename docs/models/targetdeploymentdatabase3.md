# TargetDeploymentDatabase3

## Example Usage

```typescript
import { TargetDeploymentDatabase3 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentDatabase3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                  | [models.TargetDeploymentDatabaseSecretRef3](../models/targetdeploymentdatabasesecretref3.md) | :heavy_check_mark:                                                                           | Reference to a Kubernetes Secret                                                             |