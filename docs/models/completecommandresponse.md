# CompleteCommandResponse

## Example Usage

```typescript
import { CompleteCommandResponse } from "@alienplatform/platform-api/models";

let value: CompleteCommandResponse = {
  updated: false,
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `updated`                                                          | *boolean*                                                          | :heavy_check_mark:                                                 | Whether the command transitioned; false if it was already terminal |