# OperationsPluginOperationGcp

## Example Usage

```typescript
import { OperationsPluginOperationGcp } from "@alienplatform/platform-api/models";

let value: OperationsPluginOperationGcp = {
  permissions: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  scope: "projects/${projectName}",
  reason: "<value>",
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `permissions`                                                                        | *string*[]                                                                           | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `scope`                                                                              | [models.OperationsPluginOperationScope](../models/operationspluginoperationscope.md) | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `reason`                                                                             | *string*                                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |