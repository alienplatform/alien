# TargetDeploymentTopic

## Example Usage

```typescript
import { TargetDeploymentTopic } from "@alienplatform/platform-api/models";

let value: TargetDeploymentTopic = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `secretRef`                                                                          | [models.TargetDeploymentTopicSecretRef](../models/targetdeploymenttopicsecretref.md) | :heavy_check_mark:                                                                   | Reference to a Kubernetes Secret                                                     |