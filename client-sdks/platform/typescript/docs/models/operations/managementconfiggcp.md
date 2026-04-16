# ManagementConfigGcp

GCP management configuration extracted from stack settings

## Example Usage

```typescript
import { ManagementConfigGcp } from "@alienplatform/platform-api/models/operations";

let value: ManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `serviceAccountEmail`                                                                      | *string*                                                                                   | :heavy_check_mark:                                                                         | Service account email for management roles                                                 |
| `platform`                                                                                 | [operations.CreateManagerPlatformGcp](../../models/operations/createmanagerplatformgcp.md) | :heavy_check_mark:                                                                         | N/A                                                                                        |