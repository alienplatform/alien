# ProcessMemory

## Example Usage

```typescript
import { ProcessMemory } from "@alienplatform/platform-api/models/operations";

let value: ProcessMemory = {
  unit: "requests-per-second",
  value: 9533.34,
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `unit`                                                                       | [operations.ProcessMemoryUnit](../../models/operations/processmemoryunit.md) | :heavy_check_mark:                                                           | N/A                                                                          |
| `value`                                                                      | *number*                                                                     | :heavy_check_mark:                                                           | N/A                                                                          |