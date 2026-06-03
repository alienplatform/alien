# CreateProjectAgentImageResponse

Operator image package configuration. Required when Helm is enabled. If null, Operator image packages will not be generated.

## Example Usage

```typescript
import { CreateProjectAgentImageResponse } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectAgentImageResponse = {
  displayName: "Sophie40",
  name: "<value>",
  enabled: true,
};
```

## Fields

| Field                                                     | Type                                                      | Required                                                  | Description                                               |
| --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- |
| `displayName`                                             | *string*                                                  | :heavy_check_mark:                                        | Human-friendly display name for logs and startup messages |
| `name`                                                    | *string*                                                  | :heavy_check_mark:                                        | Image name (e.g., "acme-operator")                        |
| `enabled`                                                 | *boolean*                                                 | :heavy_check_mark:                                        | Whether Operator image package generation is enabled      |