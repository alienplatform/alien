# OperationsKubernetesPermissionDiffGrant

## Example Usage

```typescript
import { OperationsKubernetesPermissionDiffGrant } from "@alienplatform/platform-api/models";

let value: OperationsKubernetesPermissionDiffGrant = {
  apiGroup: "<value>",
  resource: "<value>",
  verbs: [],
  resourceNames: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  remediationOnly: false,
  sources: [
    {
      plugin: "<value>",
      operation: "<value>",
      reason: "<value>",
    },
  ],
};
```

## Fields

| Field                                                                                                                                  | Type                                                                                                                                   | Required                                                                                                                               | Description                                                                                                                            |
| -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `apiGroup`                                                                                                                             | *string*                                                                                                                               | :heavy_check_mark:                                                                                                                     | N/A                                                                                                                                    |
| `resource`                                                                                                                             | *string*                                                                                                                               | :heavy_check_mark:                                                                                                                     | N/A                                                                                                                                    |
| `verbs`                                                                                                                                | *string*[]                                                                                                                             | :heavy_check_mark:                                                                                                                     | N/A                                                                                                                                    |
| `resourceNames`                                                                                                                        | *string*[]                                                                                                                             | :heavy_check_mark:                                                                                                                     | Empty means every resource name.                                                                                                       |
| `remediationOnly`                                                                                                                      | *boolean*                                                                                                                              | :heavy_check_mark:                                                                                                                     | True for write verbs. The Helm chart renders them only when Operator permission is set to remediation; diagnostics installs omit them. |
| `sources`                                                                                                                              | [models.OperationsPermissionDiffSource](../models/operationspermissiondiffsource.md)[]                                                 | :heavy_check_mark:                                                                                                                     | N/A                                                                                                                                    |