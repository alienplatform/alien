# DeploymentConfigDefaultDomain

## Example Usage

```typescript
import { DeploymentConfigDefaultDomain } from "@alienplatform/platform-api/models";

let value: DeploymentConfigDefaultDomain = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                          | [models.DeploymentConfigDefaultDomainSecretRef](../models/deploymentconfigdefaultdomainsecretref.md) | :heavy_check_mark:                                                                                   | Reference to a Kubernetes Secret                                                                     |