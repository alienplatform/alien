# CreateProjectOperatorImageRequest

Operator image package configuration. Required when Helm is enabled. If null, operator image packages will not be generated.

## Example Usage

```typescript
import { CreateProjectOperatorImageRequest } from "@aliendotdev/platform-api/models/operations";

let value: CreateProjectOperatorImageRequest = {
  displayName: "Sigmund_Ferry",
  name: "<value>",
  enabled: true,
};
```

## Fields

| Field                                                     | Type                                                      | Required                                                  | Description                                               |
| --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- |
| `displayName`                                             | *string*                                                  | :heavy_check_mark:                                        | Human-friendly display name for logs and startup messages |
| `name`                                                    | *string*                                                  | :heavy_check_mark:                                        | Binary name (e.g., "acme-operator")                       |
| `enabled`                                                 | *boolean*                                                 | :heavy_check_mark:                                        | Whether operator image package generation is enabled      |