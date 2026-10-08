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

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `names`                                                                  | *string*[]                                                               | :heavy_check_mark:                                                       | N/A                                                                      |
| `permissionDiff`                                                         | [models.OperationsPermissionDiff](../models/operationspermissiondiff.md) | :heavy_check_mark:                                                       | Cloud permission delta versus the previously enabled set.                |