# ResolveBindingResponsePubsub

Send-only Pubsub queue and a short-lived credential lease.

## Example Usage

```typescript
import { ResolveBindingResponsePubsub } from "@alienplatform/manager-api/models";

let value: ResolveBindingResponsePubsub = {
  binding: {
    subscription: "<value>",
    topic: "<value>",
  },
  clientConfig: {
    credentials: {
      token: "<value>",
      type: "accessToken",
    },
    projectId: "<id>",
    region: "<value>",
  },
  expiresAt: "1759634152713",
  service: "pubsub",
};
```

## Fields

| Field                                                                                                                                     | Type                                                                                                                                      | Required                                                                                                                                  | Description                                                                                                                               |
| ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                                 | [models.RemotePubsubQueueBinding](../models/remotepubsubqueuebinding.md)                                                                  | :heavy_check_mark:                                                                                                                        | Concrete send-only queue topology returned to remote clients.                                                                             |
| `clientConfig`                                                                                                                            | [models.RemoteGcpClientConfig](../models/remotegcpclientconfig.md)                                                                        | :heavy_check_mark:                                                                                                                        | Response-safe GCP client configuration. Refreshable source credentials and<br/>service endpoint overrides cannot be represented by this type. |
| `expiresAt`                                                                                                                               | *string*                                                                                                                                  | :heavy_check_mark:                                                                                                                        | N/A                                                                                                                                       |
| `service`                                                                                                                                 | *"pubsub"*                                                                                                                                | :heavy_check_mark:                                                                                                                        | N/A                                                                                                                                       |