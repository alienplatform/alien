# OperationsPermissionDiff

Cloud permission delta versus the previously enabled set.

## Example Usage

```typescript
import { OperationsPermissionDiff } from "@alienplatform/platform-api/models";

let value: OperationsPermissionDiff = {
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
};
```

## Fields

| Field                                                                                                                                                                                                                              | Type                                                                                                                                                                                                                               | Required                                                                                                                                                                                                                           | Description                                                                                                                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                                                                                                                              | [models.OperationsPermissionDiffAws](../models/operationspermissiondiffaws.md)                                                                                                                                                     | :heavy_check_mark:                                                                                                                                                                                                                 | N/A                                                                                                                                                                                                                                |
| `gcp`                                                                                                                                                                                                                              | [models.OperationsPermissionDiffGcp](../models/operationspermissiondiffgcp.md)                                                                                                                                                     | :heavy_check_mark:                                                                                                                                                                                                                 | N/A                                                                                                                                                                                                                                |
| `kubernetes`                                                                                                                                                                                                                       | [models.OperationsPermissionDiffKubernetes](../models/operationspermissiondiffkubernetes.md)                                                                                                                                       | :heavy_check_mark:                                                                                                                                                                                                                 | N/A                                                                                                                                                                                                                                |
| `incompleteForPlugins`                                                                                                                                                                                                             | *string*[]                                                                                                                                                                                                                         | :heavy_minus_sign:                                                                                                                                                                                                                 | Previously enabled plugins whose grants could not be read before this change disabled or replaced them. Added and removed grants may be incomplete; inspect existing cloud policies before applying the regenerated configuration. |