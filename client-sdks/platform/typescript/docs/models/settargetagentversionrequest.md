# SetTargetAgentVersionRequest

Set or clear the target agent version

## Example Usage

```typescript
import { SetTargetAgentVersionRequest } from "@alienplatform/platform-api/models";

let value: SetTargetAgentVersionRequest = {};
```

## Fields

| Field                                                             | Type                                                              | Required                                                          | Description                                                       |
| ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- |
| `targetAgentVersion`                                              | *string*                                                          | :heavy_minus_sign:                                                | Target agent version (semver). null or omitted clears the target. |