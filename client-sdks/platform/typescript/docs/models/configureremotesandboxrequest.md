# ConfigureRemoteSandboxRequest

## Example Usage

```typescript
import { ConfigureRemoteSandboxRequest } from "@alienplatform/platform-api/models";

let value: ConfigureRemoteSandboxRequest = {
  maxSessionLifetimeSeconds: 678048,
};
```

## Fields

| Field                       | Type                        | Required                    | Description                 |
| --------------------------- | --------------------------- | --------------------------- | --------------------------- |
| `baseImage`                 | *string*                    | :heavy_minus_sign:          | N/A                         |
| `imageBundleUri`            | *string*                    | :heavy_minus_sign:          | N/A                         |
| `maxSessionLifetimeSeconds` | *number*                    | :heavy_check_mark:          | N/A                         |