# RemoteAwsClientConfig

Response-safe AWS client configuration. The public contract deliberately
has no static, profile, metadata, or web-identity credential variants.

## Example Usage

```typescript
import { RemoteAwsClientConfig } from "@alienplatform/manager-api/models";

let value: RemoteAwsClientConfig = {
  accountId: "<id>",
  credentials: {
    accessKeyId: "<id>",
    expiresAt: "1755867390141",
    secretAccessKey: "<value>",
    sessionToken: "<value>",
    type: "sessionCredentials",
  },
  region: "<value>",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `accountId`                                                        | *string*                                                           | :heavy_check_mark:                                                 | AWS account containing the bucket.                                 |
| `credentials`                                                      | *models.RemoteAwsCredentials*                                      | :heavy_check_mark:                                                 | The only AWS credential form remote binding resolution can return. |
| `region`                                                           | *string*                                                           | :heavy_check_mark:                                                 | AWS region containing the bucket.                                  |