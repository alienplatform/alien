# PackageConfigOperatorImage

## Example Usage

```typescript
import { PackageConfigOperatorImage } from "@alienplatform/platform-api/models";

let value: PackageConfigOperatorImage = {
  displayName: "Baylee_Lubowitz",
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