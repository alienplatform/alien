# DynamicContainer

## Example Usage

```typescript
import { DynamicContainer } from "@alienplatform/platform-api/models";

let value: DynamicContainer = {
  name: "<value>",
  generation: 706060,
  status: "pending",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `name`                                                               | *string*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `generation`                                                         | *number*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `status`                                                             | [models.DynamicContainerStatus](../models/dynamiccontainerstatus.md) | :heavy_check_mark:                                                   | N/A                                                                  |
| `message`                                                            | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
