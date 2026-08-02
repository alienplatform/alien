# ResolveBindingResponseS3

AWS S3 and an AWS session.

## Example Usage

```typescript
import { ResolveBindingResponseS3 } from "@alienplatform/manager-api/models";

let value: ResolveBindingResponseS3 = {
  binding: {
    bucketName: "<value>",
  },
  clientConfig: {
    accountId: "<id>",
    credentials: {
      accessKeyId: "<id>",
      expiresAt: "1755867390141",
      secretAccessKey: "<value>",
      sessionToken: "<value>",
      type: "sessionCredentials",
    },
    region: "<value>",
  },
  expiresAt: "1750122153944",
  service: "s3",
};
```

## Fields

| Field                                                                                                                                           | Type                                                                                                                                            | Required                                                                                                                                        | Description                                                                                                                                     |
| ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                                       | [models.RemoteS3StorageBinding](../models/remotes3storagebinding.md)                                                                            | :heavy_check_mark:                                                                                                                              | Concrete S3 topology returned to remote clients.                                                                                                |
| `clientConfig`                                                                                                                                  | [models.RemoteAwsClientConfig](../models/remoteawsclientconfig.md)                                                                              | :heavy_check_mark:                                                                                                                              | Response-safe AWS client configuration. The public contract deliberately<br/>has no static, profile, metadata, or web-identity credential variants. |
| `expiresAt`                                                                                                                                     | *string*                                                                                                                                        | :heavy_check_mark:                                                                                                                              | N/A                                                                                                                                             |
| `service`                                                                                                                                       | *"s3"*                                                                                                                                          | :heavy_check_mark:                                                                                                                              | N/A                                                                                                                                             |