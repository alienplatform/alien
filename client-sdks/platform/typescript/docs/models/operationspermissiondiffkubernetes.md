# OperationsPermissionDiffKubernetes

## Example Usage

```typescript
import { OperationsPermissionDiffKubernetes } from "@alienplatform/platform-api/models";

let value: OperationsPermissionDiffKubernetes = {
  added: [],
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
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `added`                                                                                                  | [models.OperationsKubernetesPermissionDiffGrant](../models/operationskubernetespermissiondiffgrant.md)[] | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `removed`                                                                                                | [models.OperationsKubernetesPermissionDiffGrant](../models/operationskubernetespermissiondiffgrant.md)[] | :heavy_check_mark:                                                                                       | N/A                                                                                                      |