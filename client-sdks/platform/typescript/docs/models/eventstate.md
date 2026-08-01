# EventState

## Example Usage

```typescript
import { EventState } from "@alienplatform/platform-api/models";

let value: EventState = {
  failed: {},
};
```

## Fields

| Field                                | Type                                 | Required                             | Description                          |
| ------------------------------------ | ------------------------------------ | ------------------------------------ | ------------------------------------ |
| `failed`                             | [models.Failed](../models/failed.md) | :heavy_check_mark:                   | Event failed with an error           |