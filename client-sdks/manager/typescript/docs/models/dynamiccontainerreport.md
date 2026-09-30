# DynamicContainerReport

What the Operator observed after applying one target generation.

## Example Usage

```typescript
import { DynamicContainerReport } from "@alienplatform/manager-api/models";

let value: DynamicContainerReport = {
  generation: 679619,
  name: "<value>",
  status: "stopped",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `generation`                                                         | *number*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `message`                                                            | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `name`                                                               | *string*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `status`                                                             | [models.DynamicContainerStatus](../models/dynamiccontainerstatus.md) | :heavy_check_mark:                                                   | N/A                                                                  |
