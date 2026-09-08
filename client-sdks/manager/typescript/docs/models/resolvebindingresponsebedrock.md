# ResolveBindingResponseBedrock

AWS Bedrock and an AWS session.

## Example Usage

```typescript
import { ResolveBindingResponseBedrock } from "@alienplatform/manager-api/models";

let value: ResolveBindingResponseBedrock = {
  binding: {
    region: "<value>",
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
  expiresAt: "1764305704909",
  resourceId: "<id>",
  service: "bedrock",
};
```

## Fields

| Field                                                                                                                                           | Type                                                                                                                                            | Required                                                                                                                                        | Description                                                                                                                                     |
| ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                                       | [models.RemoteAwsBedrockAiBinding](../models/remoteawsbedrockaibinding.md)                                                                      | :heavy_check_mark:                                                                                                                              | N/A                                                                                                                                             |
| `clientConfig`                                                                                                                                  | [models.RemoteAwsClientConfig](../models/remoteawsclientconfig.md)                                                                              | :heavy_check_mark:                                                                                                                              | Response-safe AWS client configuration. The public contract deliberately<br/>has no static, profile, metadata, or web-identity credential variants. |
| `expiresAt`                                                                                                                                     | *string*                                                                                                                                        | :heavy_check_mark:                                                                                                                              | N/A                                                                                                                                             |
| `resourceId`                                                                                                                                    | *string*                                                                                                                                        | :heavy_check_mark:                                                                                                                              | N/A                                                                                                                                             |
| `service`                                                                                                                                       | *"bedrock"*                                                                                                                                     | :heavy_check_mark:                                                                                                                              | N/A                                                                                                                                             |