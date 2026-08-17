# DeploymentConfigTopic

## Example Usage

```typescript
import { DeploymentConfigTopic } from "@alienplatform/platform-api/models";

let value: DeploymentConfigTopic = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `secretRef`                                                                          | [models.DeploymentConfigTopicSecretRef](../models/deploymentconfigtopicsecretref.md) | :heavy_check_mark:                                                                   | Reference to a Kubernetes Secret                                                     |