# ResolveResponseManagementConfigGcp

GCP management configuration extracted from stack settings

## Example Usage

```typescript
import { ResolveResponseManagementConfigGcp } from "@alienplatform/platform-api/models";

let value: ResolveResponseManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

## Fields

| Field                                      | Type                                       | Required                                   | Description                                |
| ------------------------------------------ | ------------------------------------------ | ------------------------------------------ | ------------------------------------------ |
| `serviceAccountEmail`                      | *string*                                   | :heavy_check_mark:                         | Service account email for management roles |
| `platform`                                 | *"gcp"*                                    | :heavy_check_mark:                         | N/A                                        |