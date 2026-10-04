# ResolveBindingResponseSqs

Send-only Sqs queue and a short-lived credential lease.

## Example Usage

```typescript
import { ResolveBindingResponseSqs } from "@alienplatform/manager-api/models";

let value: ResolveBindingResponseSqs = {
  binding: {
    queueUrl: "https://caring-piglet.org",
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
  expiresAt: "1745152955992",
  service: "sqs",
};
```

## Fields

| Field                                                                                                                                           | Type                                                                                                                                            | Required                                                                                                                                        | Description                                                                                                                                     |
| ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                                       | [models.RemoteSqsQueueBinding](../models/remotesqsqueuebinding.md)                                                                              | :heavy_check_mark:                                                                                                                              | Concrete send-only queue topology returned to remote clients.                                                                                   |
| `clientConfig`                                                                                                                                  | [models.RemoteAwsClientConfig](../models/remoteawsclientconfig.md)                                                                              | :heavy_check_mark:                                                                                                                              | Response-safe AWS client configuration. The public contract deliberately<br/>has no static, profile, metadata, or web-identity credential variants. |
| `expiresAt`                                                                                                                                     | *string*                                                                                                                                        | :heavy_check_mark:                                                                                                                              | N/A                                                                                                                                             |
| `service`                                                                                                                                       | *"sqs"*                                                                                                                                         | :heavy_check_mark:                                                                                                                              | N/A                                                                                                                                             |