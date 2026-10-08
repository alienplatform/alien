# ConfigOperatorImage

Branding configuration for the Operator image.

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
| `brand`                                                   | *string*                                                  | :heavy_minus_sign:                                        | Short brand slug used for generated resource names.       |
| `displayName`                                             | *string*                                                  | :heavy_check_mark:                                        | Human-friendly display name for logs and startup messages |
| `envPrefix`                                               | *string*                                                  | :heavy_minus_sign:                                        | Branded environment variable prefix (e.g., "ACME").       |
| `labelDomain`                                             | *string*                                                  | :heavy_minus_sign:                                        | Branded Kubernetes/cloud label domain (e.g., "acme.dev"). |
| `name`                                                    | *string*                                                  | :heavy_check_mark:                                        | Image name (e.g., "acme-operator")                        |
| `type`                                                    | *"operator-image"*                                        | :heavy_check_mark:                                        | N/A                                                       |