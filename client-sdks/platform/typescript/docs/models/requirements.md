# Requirements

## Example Usage

```typescript
import { Requirements } from "@alienplatform/platform-api/models";

let value: Requirements = {
  cpu: "<value>",
  memoryBytes: 504389,
  ephemeralStorageBytes: 887074,
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `cpu`                                                  | *string*                                               | :heavy_check_mark:                                     | N/A                                                    |
| `memoryBytes`                                          | *number*                                               | :heavy_check_mark:                                     | N/A                                                    |
| `ephemeralStorageBytes`                                | *number*                                               | :heavy_check_mark:                                     | N/A                                                    |
| `gpu`                                                  | [models.RequirementsGpu](../models/requirementsgpu.md) | :heavy_minus_sign:                                     | N/A                                                    |