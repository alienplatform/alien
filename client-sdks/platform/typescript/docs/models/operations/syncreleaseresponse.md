# SyncReleaseResponse

Lock released successfully.

## Example Usage

```typescript
import { SyncReleaseResponse } from "@alienplatform/platform-api/models/operations";

let value: SyncReleaseResponse = {
  success: true,
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `success`                                                                                                          | *boolean*                                                                                                          | :heavy_check_mark:                                                                                                 | True when released or the exact terminal receipt was acknowledged. False when completion could not be established. |
