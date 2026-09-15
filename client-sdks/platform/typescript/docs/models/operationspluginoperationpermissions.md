# OperationsPluginOperationPermissions

Cloud permissions required to execute this operation.

## Example Usage

```typescript
import { OperationsPluginOperationPermissions } from "@alienplatform/platform-api/models";

let value: OperationsPluginOperationPermissions = {
  azure: [],
  aws: [],
  gcp: [
    {
      permissions: [
        "<value 1>",
      ],
      scope: "projects/${projectName}",
      reason: "<value>",
    },
  ],
};
```

## Fields

| Field                                                                                 | Type                                                                                  | Required                                                                              | Description                                                                           |
| ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| `azure`                                                                               | *string*[]                                                                            | :heavy_check_mark:                                                                    | No Azure resource permissions are supported. Kubernetes API permissions are separate. |
| `aws`                                                                                 | [models.OperationsPluginOperationAw](../models/operationspluginoperationaw.md)[]      | :heavy_check_mark:                                                                    | N/A                                                                                   |
| `gcp`                                                                                 | [models.OperationsPluginOperationGcp](../models/operationspluginoperationgcp.md)[]    | :heavy_check_mark:                                                                    | N/A                                                                                   |