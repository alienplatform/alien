# Arc

## Example Usage

```typescript
import { Arc } from "@aliendotdev/platform-api/models";

let value: Arc = {
  url: "https://simple-impact.info/",
  deploymentId: "<id>",
};
```

## Fields

| Field                              | Type                               | Required                           | Description                        |
| ---------------------------------- | ---------------------------------- | ---------------------------------- | ---------------------------------- |
| `url`                              | *string*                           | :heavy_check_mark:                 | Agent Manager URL for ARC commands |
| `deploymentId`                     | *string*                           | :heavy_check_mark:                 | Agent ID to use in ARC requests    |