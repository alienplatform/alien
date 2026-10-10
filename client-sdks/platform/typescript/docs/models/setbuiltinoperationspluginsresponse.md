# SetBuiltinOperationsPluginsResponse

## Example Usage

```typescript
import { SetBuiltinOperationsPluginsResponse } from "@alienplatform/platform-api/models";

let value: SetBuiltinOperationsPluginsResponse = {
  names: [
    "<value 1>",
  ],
  permissionDiff: {
    aws: {
      added: [
        {
          effect: "Deny",
          actions: [
            "<value 1>",
            "<value 2>",
            "<value 3>",
          ],
          resources: [],
          condition: {
            "key": {
              "key": "<value>",
              "key1": "<value>",
            },
            "key1": {
              "key": "<value>",
            },
            "key2": {
              "key": "<value>",
            },
          },
          sources: [],
        },
      ],
      removed: [],
    },
    gcp: {
      added: [],
      removed: [
        {
          permission: "<value>",
          scope: "projects/${projectName}/buckets/${resourceName}",
          sources: [
            {
              plugin: "<value>",
              operation: "<value>",
              reason: "<value>",
            },
          ],
        },
      ],
    },
    kubernetes: {
      added: [
        {
          apiGroup: "<value>",
          resource: "<value>",
          verbs: [
            "<value 1>",
            "<value 2>",
          ],
          resourceNames: [
            "<value 1>",
            "<value 2>",
          ],
          remediationOnly: true,
          sources: [
            {
              plugin: "<value>",
              operation: "<value>",
              reason: "<value>",
            },
          ],
        },
      ],
      removed: [
        {
          apiGroup: "<value>",
          resource: "<value>",
          verbs: [
            "<value 1>",
            "<value 2>",
          ],
          resourceNames: [],
          remediationOnly: true,
          sources: [],
        },
      ],
    },
  },
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `names`                                                                  | *string*[]                                                               | :heavy_check_mark:                                                       | N/A                                                                      |
| `permissionDiff`                                                         | [models.OperationsPermissionDiff](../models/operationspermissiondiff.md) | :heavy_check_mark:                                                       | Cloud permission delta versus the previously enabled set.                |