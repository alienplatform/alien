# RemoteGcpClientConfig

Response-safe GCP client configuration. Refreshable source credentials and
service endpoint overrides cannot be represented by this type.

## Example Usage

```typescript
import { RemoteGcpClientConfig } from "@alienplatform/manager-api/models";

let value: RemoteGcpClientConfig = {
  credentials: {
    token: "<value>",
    type: "accessToken",
  },
  projectId: "<id>",
  region: "<value>",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `credentials`                                                      | *models.RemoteGcpCredentials*                                      | :heavy_check_mark:                                                 | The only GCP credential form remote binding resolution can return. |
| `projectId`                                                        | *string*                                                           | :heavy_check_mark:                                                 | GCP project containing the bucket.                                 |
| `projectNumber`                                                    | *string*                                                           | :heavy_minus_sign:                                                 | Numeric GCP project id, when known.                                |
| `region`                                                           | *string*                                                           | :heavy_check_mark:                                                 | GCP region configured for the deployment.                          |