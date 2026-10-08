# Aw

## Example Usage

```typescript
import { Aw } from "@alienplatform/platform-api/models/operations";

let value: Aw = {
  effect: "Allow",
  actions: [
    "<value 1>",
  ],
  resources: [],
  condition: null,
  reason: "<value>",
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `effect`                                               | [operations.Effect](../../models/operations/effect.md) | :heavy_check_mark:                                     | N/A                                                    |
| `actions`                                              | *string*[]                                             | :heavy_check_mark:                                     | N/A                                                    |
| `resources`                                            | *string*[]                                             | :heavy_check_mark:                                     | N/A                                                    |
| `condition`                                            | Record<string, Record<string, *string*>>               | :heavy_check_mark:                                     | N/A                                                    |
| `reason`                                               | *string*                                               | :heavy_check_mark:                                     | N/A                                                    |