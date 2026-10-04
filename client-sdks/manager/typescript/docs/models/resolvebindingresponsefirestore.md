# ResolveBindingResponseFirestore

GCP Firestore KV collection and an access token.

## Example Usage

```typescript
import { ResolveBindingResponseFirestore } from "@alienplatform/manager-api/models";

let value: ResolveBindingResponseFirestore = {
  binding: {
    collectionName: "<value>",
    databaseId: "<id>",
    projectId: "<id>",
  },
  clientConfig: {
    credentials: {
      token: "<value>",
      type: "accessToken",
    },
    projectId: "<id>",
    region: "<value>",
  },
  expiresAt: "1736089335940",
  service: "firestore",
};
```

## Fields

| Field                                                                                                                                     | Type                                                                                                                                      | Required                                                                                                                                  | Description                                                                                                                               |
| ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                                 | [models.RemoteFirestoreKvBinding](../models/remotefirestorekvbinding.md)                                                                  | :heavy_check_mark:                                                                                                                        | Concrete Firestore KV topology returned to remote clients.                                                                                |
| `clientConfig`                                                                                                                            | [models.RemoteGcpClientConfig](../models/remotegcpclientconfig.md)                                                                        | :heavy_check_mark:                                                                                                                        | Response-safe GCP client configuration. Refreshable source credentials and<br/>service endpoint overrides cannot be represented by this type. |
| `expiresAt`                                                                                                                               | *string*                                                                                                                                  | :heavy_check_mark:                                                                                                                        | N/A                                                                                                                                       |
| `service`                                                                                                                                 | *"firestore"*                                                                                                                             | :heavy_check_mark:                                                                                                                        | N/A                                                                                                                                       |