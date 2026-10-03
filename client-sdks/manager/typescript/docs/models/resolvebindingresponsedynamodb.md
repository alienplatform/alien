# ResolveBindingResponseDynamodb

AWS DynamoDB KV table and an AWS session.

## Example Usage

```typescript
import { ResolveBindingResponseDynamodb } from "@alienplatform/manager-api/models";

let value: ResolveBindingResponseDynamodb = {
  binding: {
    region: "<value>",
    tableName: "<value>",
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
  expiresAt: "1760103754317",
  service: "dynamodb",
};
```

## Fields

| Field                                                                                                                                           | Type                                                                                                                                            | Required                                                                                                                                        | Description                                                                                                                                     |
| ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                                       | [models.RemoteDynamodbKvBinding](../models/remotedynamodbkvbinding.md)                                                                          | :heavy_check_mark:                                                                                                                              | Concrete DynamoDB KV topology returned to remote clients.                                                                                       |
| `clientConfig`                                                                                                                                  | [models.RemoteAwsClientConfig](../models/remoteawsclientconfig.md)                                                                              | :heavy_check_mark:                                                                                                                              | Response-safe AWS client configuration. The public contract deliberately<br/>has no static, profile, metadata, or web-identity credential variants. |
| `expiresAt`                                                                                                                                     | *string*                                                                                                                                        | :heavy_check_mark:                                                                                                                              | N/A                                                                                                                                             |
| `service`                                                                                                                                       | *"dynamodb"*                                                                                                                                    | :heavy_check_mark:                                                                                                                              | N/A                                                                                                                                             |