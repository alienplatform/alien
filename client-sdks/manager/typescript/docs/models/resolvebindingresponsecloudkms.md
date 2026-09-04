# ResolveBindingResponseCloudKms

GCP Cloud KMS key and an access token.

## Example Usage

```typescript
import { ResolveBindingResponseCloudKms } from "@alienplatform/manager-api/models";

let value: ResolveBindingResponseCloudKms = {
  binding: {
    cryptoKeyName: "<value>",
  },
  clientConfig: {
    credentials: {
      token: "<value>",
      type: "accessToken",
    },
    projectId: "<id>",
    region: "<value>",
  },
  expiresAt: "1760734762170",
  service: "cloud-kms",
};
```

## Fields

| Field                                                                                                                                     | Type                                                                                                                                      | Required                                                                                                                                  | Description                                                                                                                               |
| ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                                 | [models.RemoteGcpCloudKmsKeyBinding](../models/remotegcpcloudkmskeybinding.md)                                                            | :heavy_check_mark:                                                                                                                        | N/A                                                                                                                                       |
| `clientConfig`                                                                                                                            | [models.RemoteGcpClientConfig](../models/remotegcpclientconfig.md)                                                                        | :heavy_check_mark:                                                                                                                        | Response-safe GCP client configuration. Refreshable source credentials and<br/>service endpoint overrides cannot be represented by this type. |
| `expiresAt`                                                                                                                               | *string*                                                                                                                                  | :heavy_check_mark:                                                                                                                        | N/A                                                                                                                                       |
| `service`                                                                                                                                 | *"cloud-kms"*                                                                                                                             | :heavy_check_mark:                                                                                                                        | N/A                                                                                                                                       |