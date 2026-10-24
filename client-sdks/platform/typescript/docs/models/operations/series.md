# Series

## Example Usage

```typescript
import { Series } from "@alienplatform/platform-api/models/operations";

let value: Series = {
  resourceId: "<id>",
  replicaId: "<id>",
  points: [],
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `resourceId`                                           | *string*                                               | :heavy_check_mark:                                     | N/A                                                    |
| `replicaId`                                            | *string*                                               | :heavy_check_mark:                                     | N/A                                                    |
| `points`                                               | [operations.Point](../../models/operations/point.md)[] | :heavy_check_mark:                                     | N/A                                                    |