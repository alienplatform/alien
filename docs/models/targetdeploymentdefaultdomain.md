# TargetDeploymentDefaultDomain

## Example Usage

```typescript
import { TargetDeploymentDefaultDomain } from "@alienplatform/platform-api/models";

let value: TargetDeploymentDefaultDomain = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                          | [models.TargetDeploymentDefaultDomainSecretRef](../models/targetdeploymentdefaultdomainsecretref.md) | :heavy_check_mark:                                                                                   | Reference to a Kubernetes Secret                                                                     |