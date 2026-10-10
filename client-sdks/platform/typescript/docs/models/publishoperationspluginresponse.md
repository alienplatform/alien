# PublishOperationsPluginResponse

## Example Usage

```typescript
import { PublishOperationsPluginResponse } from "@alienplatform/platform-api/models";

let value: PublishOperationsPluginResponse = {
  name: "<value>",
  version: "<value>",
  tier: "read-only",
  enabled: false,
  permissionDiff: {
    aws: {
      added: [],
      removed: [
        {
          effect: "Allow",
          actions: [
            "<value 1>",
            "<value 2>",
          ],
          resources: [
            "<value 1>",
          ],
          condition: {},
          sources: [],
        },
      ],
    },
    gcp: {
      added: [],
      removed: [],
    },
    kubernetes: {
      added: [],
      removed: [
        {
          apiGroup: "<value>",
          resource: "<value>",
          verbs: [],
          resourceNames: [],
          remediationOnly: false,
          sources: [],
        },
      ],
    },
  },
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `name`                                                                                         | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `version`                                                                                      | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `tier`                                                                                         | [models.PublishOperationsPluginResponseTier](../models/publishoperationspluginresponsetier.md) | :heavy_check_mark:                                                                             | How risky an operation is (declared by the plugin metadata).                                   |
| `enabled`                                                                                      | *boolean*                                                                                      | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `permissionDiff`                                                                               | [models.OperationsPermissionDiff](../models/operationspermissiondiff.md)                       | :heavy_check_mark:                                                                             | Cloud permission delta versus the previously enabled set.                                      |