# DeploymentInfoInstallContext

## Example Usage

```typescript
import { DeploymentInfoInstallContext } from "@alienplatform/platform-api/models";

let value: DeploymentInfoInstallContext = {
  targets: {},
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `targets`                                                        | Record<string, [models.Targets](../models/targets.md)>           | :heavy_check_mark:                                               | Deployment-session install context by Terraform/installer target |