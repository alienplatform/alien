# PackagesConfigPatchOperatorImage

## Example Usage

```typescript
import { PackagesConfigPatchOperatorImage } from "@alienplatform/platform-api/models";

let value: PackagesConfigPatchOperatorImage = {};
```

## Fields

| Field                                                     | Type                                                      | Required                                                  | Description                                               |
| --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- |
| `brand`                                                   | *string*                                                  | :heavy_minus_sign:                                        | Short brand slug used for generated resource names.       |
| `displayName`                                             | *string*                                                  | :heavy_minus_sign:                                        | Human-friendly display name for logs and startup messages |
| `envPrefix`                                               | *string*                                                  | :heavy_minus_sign:                                        | Branded environment variable prefix (e.g., "ACME").       |
| `labelDomain`                                             | *string*                                                  | :heavy_minus_sign:                                        | Branded Kubernetes/cloud label domain (e.g., "acme.dev"). |
| `name`                                                    | *string*                                                  | :heavy_minus_sign:                                        | Image name (e.g., "acme-operator")                        |
| `enabled`                                                 | *boolean*                                                 | :heavy_minus_sign:                                        | Whether Operator image package generation is enabled      |