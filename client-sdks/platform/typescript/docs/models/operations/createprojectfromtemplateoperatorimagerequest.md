# CreateProjectFromTemplateOperatorImageRequest

Operator image package configuration. Required when Helm is enabled. If null, Operator image packages will not be generated.

## Example Usage

```typescript
import { CreateProjectFromTemplateOperatorImageRequest } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectFromTemplateOperatorImageRequest = {
  displayName: "Gregg_Blick48",
  name: "<value>",
  enabled: false,
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
| `enabled`                                                 | *boolean*                                                 | :heavy_check_mark:                                        | Whether Operator image package generation is enabled      |