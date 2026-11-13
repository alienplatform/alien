# ValueSlotsResponse

## Example Usage

```typescript
import { ValueSlotsResponse } from "@alienplatform/manager-api/models";

let value: ValueSlotsResponse = {
  slots: [
    {
      digest: "<value>",
      kind: "plain",
      slot: "<value>",
      updatedAt: "1735624454389",
    },
  ],
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `slots`                                                | [models.ValueSlotEntry](../models/valueslotentry.md)[] | :heavy_check_mark:                                     | N/A                                                    |
