# ConfigAgentImage

Branding configuration for the Operator image.

## Example Usage

```typescript
import { ConfigAgentImage } from "@alienplatform/platform-api/models";

let value: ConfigAgentImage = {
  displayName: "Jack.Kemmer87",
  name: "<value>",
  type: "agent-image",
};
```

## Fields

| Field                                                     | Type                                                      | Required                                                  | Description                                               |
| --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- |
| `displayName`                                             | *string*                                                  | :heavy_check_mark:                                        | Human-friendly display name for logs and startup messages |
| `name`                                                    | *string*                                                  | :heavy_check_mark:                                        | Image name (e.g., "acme-operator")                        |
| `type`                                                    | *"agent-image"*                                           | :heavy_check_mark:                                        | N/A                                                       |