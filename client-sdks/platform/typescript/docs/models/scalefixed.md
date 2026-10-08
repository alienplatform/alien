# ScaleFixed

## Example Usage

```typescript
import { ScaleFixed } from "@alienplatform/platform-api/models";

let value: ScaleFixed = {
  type: "fixed",
  machines: {
    min: 416298,
    max: 241851,
    default: 981503,
  },
};
```

## Fields

| Field                                    | Type                                     | Required                                 | Description                              |
| ---------------------------------------- | ---------------------------------------- | ---------------------------------------- | ---------------------------------------- |
| `type`                                   | *"fixed"*                                | :heavy_check_mark:                       | N/A                                      |
| `machines`                               | [models.Machines](../models/machines.md) | :heavy_check_mark:                       | N/A                                      |