# ImportSourceManagementConfigGcp

GCP management configuration extracted from stack settings

## Example Usage

```typescript
import { ImportSourceManagementConfigGcp } from "@alienplatform/platform-api/models";

let value: ImportSourceManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

## Fields

| Field                                      | Type                                       | Required                                   | Description                                |
| ------------------------------------------ | ------------------------------------------ | ------------------------------------------ | ------------------------------------------ |
| `serviceAccountEmail`                      | *string*                                   | :heavy_check_mark:                         | Service account email for management roles |
| `platform`                                 | *"gcp"*                                    | :heavy_check_mark:                         | N/A                                        |