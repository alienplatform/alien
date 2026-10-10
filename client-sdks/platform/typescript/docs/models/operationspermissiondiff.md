# OperationsPermissionDiff

Cloud permission delta versus the previously enabled set.

## Example Usage

```typescript
import { OperationsPermissionDiff } from "@alienplatform/platform-api/models";

let value: OperationsPermissionDiff = {
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
};
```

## Fields

| Field                                                                                                                                                                                                                              | Type                                                                                                                                                                                                                               | Required                                                                                                                                                                                                                           | Description                                                                                                                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                                                                                                                              | [models.OperationsPermissionDiffAws](../models/operationspermissiondiffaws.md)                                                                                                                                                     | :heavy_check_mark:                                                                                                                                                                                                                 | N/A                                                                                                                                                                                                                                |
| `gcp`                                                                                                                                                                                                                              | [models.OperationsPermissionDiffGcp](../models/operationspermissiondiffgcp.md)                                                                                                                                                     | :heavy_check_mark:                                                                                                                                                                                                                 | N/A                                                                                                                                                                                                                                |
| `kubernetes`                                                                                                                                                                                                                       | [models.OperationsPermissionDiffKubernetes](../models/operationspermissiondiffkubernetes.md)                                                                                                                                       | :heavy_check_mark:                                                                                                                                                                                                                 | N/A                                                                                                                                                                                                                                |
| `incompleteForPlugins`                                                                                                                                                                                                             | *string*[]                                                                                                                                                                                                                         | :heavy_minus_sign:                                                                                                                                                                                                                 | Previously enabled plugins whose grants could not be read before this change disabled or replaced them. Added and removed grants may be incomplete; inspect existing cloud policies before applying the regenerated configuration. |