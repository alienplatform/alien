# ConfigureProjectSourceRemoteSandbox

## Example Usage

```typescript
import { ConfigureProjectSourceRemoteSandbox } from "@alienplatform/platform-api/models/operations";

let value: ConfigureProjectSourceRemoteSandbox = {
  enabled: true,
  maxSessionLifetimeSeconds: 451760,
};
```

## Fields

| Field                       | Type                        | Required                    | Description                 |
| --------------------------- | --------------------------- | --------------------------- | --------------------------- |
| `enabled`                   | *boolean*                   | :heavy_check_mark:          | N/A                         |
| `baseImage`                 | *string*                    | :heavy_minus_sign:          | N/A                         |
| `imageBundleUri`            | *string*                    | :heavy_minus_sign:          | N/A                         |
| `maxSessionLifetimeSeconds` | *number*                    | :heavy_check_mark:          | N/A                         |