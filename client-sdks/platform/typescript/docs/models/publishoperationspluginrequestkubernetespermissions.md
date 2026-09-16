# PublishOperationsPluginRequestKubernetesPermissions

## Example Usage

```typescript
import { PublishOperationsPluginRequestKubernetesPermissions } from "@alienplatform/platform-api/models";

let value: PublishOperationsPluginRequestKubernetesPermissions = {
  schemaVersion: 9089.49,
  rules: [
    {
      apiGroup: "<value>",
      resource: "<value>",
      verbs: [
        "watch",
      ],
      reason: "<value>",
    },
  ],
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `schemaVersion`                                                                                | *number*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `rules`                                                                                        | [models.PublishOperationsPluginRequestRule](../models/publishoperationspluginrequestrule.md)[] | :heavy_check_mark:                                                                             | N/A                                                                                            |