# ResolveBindingResponseKms

AWS KMS key and an AWS session.

## Example Usage

```typescript
import { ResolveBindingResponseKms } from "@alienplatform/manager-api/models";

let value: ResolveBindingResponseKms = {
  binding: {
    keyArn: "<value>",
  },
  clientConfig: {
    accountId: "<id>",
    credentials: {
      accessKeyId: "<id>",
      expiresAt: "1744601542027",
      secretAccessKey: "<value>",
      sessionToken: "<value>",
      type: "sessionCredentials",
    },
    region: "<value>",
  },
  expiresAt: "1738330324847",
  service: "kms",
};
```

## Fields

| Field                                                                                                                                           | Type                                                                                                                                            | Required                                                                                                                                        | Description                                                                                                                                     |
| ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                                       | [models.RemoteAwsKmsKeyBinding](../models/remoteawskmskeybinding.md)                                                                            | :heavy_check_mark:                                                                                                                              | N/A                                                                                                                                             |
| `clientConfig`                                                                                                                                  | [models.RemoteAwsClientConfig](../models/remoteawsclientconfig.md)                                                                              | :heavy_check_mark:                                                                                                                              | Response-safe AWS client configuration. The public contract deliberately<br/>has no static, profile, metadata, or web-identity credential variants. |
| `expiresAt`                                                                                                                                     | *string*                                                                                                                                        | :heavy_check_mark:                                                                                                                              | N/A                                                                                                                                             |
| `service`                                                                                                                                       | *"kms"*                                                                                                                                         | :heavy_check_mark:                                                                                                                              | N/A                                                                                                                                             |