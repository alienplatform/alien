# ClientConfigGcp

GCP client configuration

## Example Usage

```typescript
import { ClientConfigGcp } from "@alienplatform/manager-api/models";

let value: ClientConfigGcp = {
  credentials: {
    config: {
      scopes: [
        "<value 1>",
      ],
      serviceAccountEmail: "<value>",
    },
    source: {},
    type: "impersonatedServiceAccount",
  },
  projectId: "<id>",
  region: "<value>",
  platform: "gcp",
};
```

## Fields

| Field                                                                                                                                                      | Type                                                                                                                                                       | Required                                                                                                                                                   | Description                                                                                                                                                |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `credentials`                                                                                                                                              | *models.GcpCredentials*                                                                                                                                    | :heavy_check_mark:                                                                                                                                         | Authentication options for talking to GCP APIs.                                                                                                            |
| `projectId`                                                                                                                                                | *string*                                                                                                                                                   | :heavy_check_mark:                                                                                                                                         | The GCP Project ID.                                                                                                                                        |
| `projectNumber`                                                                                                                                            | *string*                                                                                                                                                   | :heavy_minus_sign:                                                                                                                                         | The GCP project number (numeric). Resolved at runtime via Resource Manager API.<br/>Used in IAM condition expressions where resource.name uses project number. |
| `region`                                                                                                                                                   | *string*                                                                                                                                                   | :heavy_check_mark:                                                                                                                                         | The GCP region for resources.                                                                                                                              |
| `serviceOverrides`                                                                                                                                         | [models.GcpServiceOverrides](../models/gcpserviceoverrides.md)                                                                                             | :heavy_minus_sign:                                                                                                                                         | N/A                                                                                                                                                        |
| `platform`                                                                                                                                                 | [models.ClientConfigPlatformGcp](../models/clientconfigplatformgcp.md)                                                                                     | :heavy_check_mark:                                                                                                                                         | N/A                                                                                                                                                        |