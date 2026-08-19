# DeploymentConfigQueueName

## Example Usage

```typescript
import { DeploymentConfigQueueName } from "@alienplatform/platform-api/models";

let value: DeploymentConfigQueueName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                  | [models.DeploymentConfigQueueNameSecretRef](../models/deploymentconfigqueuenamesecretref.md) | :heavy_check_mark:                                                                           | Reference to a Kubernetes Secret                                                             |