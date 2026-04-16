# BodySpecStorage

Storage-backed body

## Example Usage

```typescript
import { BodySpecStorage } from "@alienplatform/manager-api/models";

let value: BodySpecStorage = {
  mode: "storage",
};
```

## Fields

| Field                                                     | Type                                                      | Required                                                  | Description                                               |
| --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- |
| `mode`                                                    | *"storage"*                                               | :heavy_check_mark:                                        | N/A                                                       |
| `size`                                                    | *number*                                                  | :heavy_minus_sign:                                        | Size of the body in bytes                                 |
| `storageGetRequest`                                       | [models.PresignedRequest](../models/presignedrequest.md)  | :heavy_minus_sign:                                        | N/A                                                       |
| `storagePutUsed`                                          | *boolean*                                                 | :heavy_minus_sign:                                        | Indicates storage upload was used for response submission |