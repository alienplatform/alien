# PresignedRequestBackendLocal

Local filesystem operation

## Example Usage

```typescript
import { PresignedRequestBackendLocal } from "@alienplatform/manager-api/models";

let value: PresignedRequestBackendLocal = {
  filePath: "/Users/seafood.bz",
  operation: "get",
  type: "local",
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `filePath`                                           | *string*                                             | :heavy_check_mark:                                   | N/A                                                  |
| `operation`                                          | [models.LocalOperation](../models/localoperation.md) | :heavy_check_mark:                                   | Local filesystem operations                          |
| `type`                                               | *"local"*                                            | :heavy_check_mark:                                   | N/A                                                  |