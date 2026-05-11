# InstallContext

## Example Usage

```typescript
import { InstallContext } from "@alienplatform/platform-api/models";

let value: InstallContext = {
  targets: {
    "key": {
      platform: "kubernetes",
      managerUrl: "https://excitable-drug.name/",
      managementConfig: {
        managingTenantId: "<id>",
        platform: "azure",
      },
    },
  },
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `targets`                                                        | Record<string, [models.Targets](../models/targets.md)>           | :heavy_check_mark:                                               | Deployment-session install context by Terraform/installer target |