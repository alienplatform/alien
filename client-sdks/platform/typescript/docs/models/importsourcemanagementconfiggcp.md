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

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `federationTrust`                                                      | *models.ImportSourceFederationTrustUnion*                              | :heavy_minus_sign:                                                     | N/A                                                                    |
| `serviceAccountEmail`                                                  | *string*                                                               | :heavy_check_mark:                                                     | Linked source account metadata; never a direct target trust grant.     |
| `platform`                                                             | [models.ImportSourcePlatformGcp](../models/importsourceplatformgcp.md) | :heavy_check_mark:                                                     | N/A                                                                    |