# ResolveBindingResponseVertex

GCP Vertex AI and an access token.

## Example Usage

```typescript
import { ResolveBindingResponseVertex } from "@alienplatform/manager-api/models";

let value: ResolveBindingResponseVertex = {
  binding: {
    location: "<value>",
    project: "<value>",
  },
  clientConfig: {
    credentials: {
      token: "<value>",
      type: "accessToken",
    },
    projectId: "<id>",
    region: "<value>",
  },
  expiresAt: "1739955288025",
  resourceId: "<id>",
  service: "vertex",
};
```

## Fields

| Field                                                                                                                                     | Type                                                                                                                                      | Required                                                                                                                                  | Description                                                                                                                               |
| ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                                 | [models.RemoteGcpVertexAiBinding](../models/remotegcpvertexaibinding.md)                                                                  | :heavy_check_mark:                                                                                                                        | N/A                                                                                                                                       |
| `clientConfig`                                                                                                                            | [models.RemoteGcpClientConfig](../models/remotegcpclientconfig.md)                                                                        | :heavy_check_mark:                                                                                                                        | Response-safe GCP client configuration. Refreshable source credentials and<br/>service endpoint overrides cannot be represented by this type. |
| `expiresAt`                                                                                                                               | *string*                                                                                                                                  | :heavy_check_mark:                                                                                                                        | N/A                                                                                                                                       |
| `resourceId`                                                                                                                              | *string*                                                                                                                                  | :heavy_check_mark:                                                                                                                        | N/A                                                                                                                                       |
| `service`                                                                                                                                 | *"vertex"*                                                                                                                                | :heavy_check_mark:                                                                                                                        | N/A                                                                                                                                       |