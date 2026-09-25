# OperationsPermissionDiffSource

## Example Usage

```typescript
import { OperationsPermissionDiffSource } from "@alienplatform/platform-api/models";

let value: OperationsPermissionDiffSource = {
  plugin: "<value>",
  operation: "<value>",
  reason: "<value>",
};
```

## Fields

| Field                                    | Type                                     | Required                                 | Description                              |
| ---------------------------------------- | ---------------------------------------- | ---------------------------------------- | ---------------------------------------- |
| `plugin`                                 | *string*                                 | :heavy_check_mark:                       | Plugin that declared the permission.     |
| `operation`                              | *string*                                 | :heavy_check_mark:                       | Operation that declared the permission.  |
| `reason`                                 | *string*                                 | :heavy_check_mark:                       | Why the operation needs this permission. |