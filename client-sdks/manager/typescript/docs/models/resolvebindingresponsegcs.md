# ResolveBindingResponseGcs

Google Cloud Storage and a bucket-downscoped access token.

## Example Usage

```typescript
import { ResolveBindingResponseGcs } from "@alienplatform/manager-api/models";

let value: ResolveBindingResponseGcs = {
  binding: {
    bucketName: "<value>",
  },
  clientConfig: {
    credentials: {
      token: "<value>",
      type: "accessToken",
    },
    projectId: "<id>",
    region: "<value>",
  },
  expiresAt: "1741179780880",
  service: "gcs",
};
```

## Fields

| Field                                                                                                                                     | Type                                                                                                                                      | Required                                                                                                                                  | Description                                                                                                                               |
| ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                                 | [models.RemoteGcsStorageBinding](../models/remotegcsstoragebinding.md)                                                                    | :heavy_check_mark:                                                                                                                        | Concrete Google Cloud Storage topology returned to remote clients.                                                                        |
| `clientConfig`                                                                                                                            | [models.RemoteGcpClientConfig](../models/remotegcpclientconfig.md)                                                                        | :heavy_check_mark:                                                                                                                        | Response-safe GCP client configuration. Refreshable source credentials and<br/>service endpoint overrides cannot be represented by this type. |
| `expiresAt`                                                                                                                               | *string*                                                                                                                                  | :heavy_check_mark:                                                                                                                        | N/A                                                                                                                                       |
| `service`                                                                                                                                 | *"gcs"*                                                                                                                                   | :heavy_check_mark:                                                                                                                        | N/A                                                                                                                                       |