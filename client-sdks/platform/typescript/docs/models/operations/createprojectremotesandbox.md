# CreateProjectRemoteSandbox

## Example Usage

```typescript
import { CreateProjectRemoteSandbox } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectRemoteSandbox = {
  enabled: false,
  maxSessionLifetimeSeconds: 870271,
};
```

## Fields

| Field                       | Type                        | Required                    | Description                 |
| --------------------------- | --------------------------- | --------------------------- | --------------------------- |
| `enabled`                   | *boolean*                   | :heavy_check_mark:          | N/A                         |
| `baseImage`                 | *string*                    | :heavy_minus_sign:          | N/A                         |
| `imageBundleUri`            | *string*                    | :heavy_minus_sign:          | N/A                         |
| `maxSessionLifetimeSeconds` | *number*                    | :heavy_check_mark:          | N/A                         |