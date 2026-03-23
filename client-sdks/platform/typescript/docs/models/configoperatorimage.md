# ConfigOperatorImage

Configuration for the Operator binary

## Example Usage

```typescript
import { ConfigOperatorImage } from "@alienplatform/platform-api/models";

let value: ConfigOperatorImage = {
  displayName: "Gilda_Gibson",
  name: "<value>",
  type: "operator-image",
};
```

## Fields

| Field                                                     | Type                                                      | Required                                                  | Description                                               |
| --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- |
| `displayName`                                             | *string*                                                  | :heavy_check_mark:                                        | Human-friendly display name for logs and startup messages |
| `name`                                                    | *string*                                                  | :heavy_check_mark:                                        | Binary name (e.g., "acme-operator")                       |
| `type`                                                    | *"operator-image"*                                        | :heavy_check_mark:                                        | N/A                                                       |