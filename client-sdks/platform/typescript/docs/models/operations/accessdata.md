# AccessData

## Example Usage

```typescript
import { AccessData } from "@alienplatform/platform-api/models/operations";

let value: AccessData = {
  pending: 915055,
  active: 749749,
  recent: [],
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `pending`                                                            | *number*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `active`                                                             | *number*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `recent`                                                             | [operations.AccessRecent](../../models/operations/accessrecent.md)[] | :heavy_check_mark:                                                   | N/A                                                                  |