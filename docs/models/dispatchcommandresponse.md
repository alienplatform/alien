# DispatchCommandResponse

## Example Usage

```typescript
import { DispatchCommandResponse } from "@alienplatform/platform-api/models";

let value: DispatchCommandResponse = {
  updated: true,
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `updated`                                                          | *boolean*                                                          | :heavy_check_mark:                                                 | Whether the command transitioned; false if it was already terminal |