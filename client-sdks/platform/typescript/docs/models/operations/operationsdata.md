# OperationsData

## Example Usage

```typescript
import { OperationsData } from "@alienplatform/platform-api/models/operations";

let value: OperationsData = {
  selected: [],
  sync: {
    preparing: 460444,
    syncing: 482425,
    ready: 229664,
    stuck: 429061,
  },
  generatedPermissions: [],
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `selected`                                                                         | [operations.Selected](../../models/operations/selected.md)[]                       | :heavy_check_mark:                                                                 | N/A                                                                                |
| `sync`                                                                             | [operations.Sync](../../models/operations/sync.md)                                 | :heavy_check_mark:                                                                 | N/A                                                                                |
| `generatedPermissions`                                                             | [operations.GeneratedPermission](../../models/operations/generatedpermission.md)[] | :heavy_check_mark:                                                                 | N/A                                                                                |