# ResolveBindingResponseSandboxGcpAgentPlatform

GCP Agent Platform reasoning engine and a GCP access token.

## Example Usage

```typescript
import { ResolveBindingResponseSandboxGcpAgentPlatform } from "@alienplatform/manager-api/models";

let value: ResolveBindingResponseSandboxGcpAgentPlatform = {
  binding: {
    engine: "<value>",
    region: "<value>",
    template: "<value>",
  },
  clientConfig: {
    credentials: {
      token: "<value>",
      type: "accessToken",
    },
    projectId: "<id>",
    region: "<value>",
  },
  expiresAt: "1749791336160",
  service: "sandbox-gcp-agent-platform",
};
```

## Fields

| Field                                                                                                                                                                                                                | Type                                                                                                                                                                                                                 | Required                                                                                                                                                                                                             | Description                                                                                                                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                                                                                                            | [models.RemoteGcpSandboxBinding](../models/remotegcpsandboxbinding.md)                                                                                                                                               | :heavy_check_mark:                                                                                                                                                                                                   | Concrete Agent Platform topology returned to remote clients.<br/><br/>The egress policy itself lives on the environment template named below; `allow_egress` reports<br/>it so a client can decide without reading the template. |
| `clientConfig`                                                                                                                                                                                                       | [models.RemoteGcpClientConfig](../models/remotegcpclientconfig.md)                                                                                                                                                   | :heavy_check_mark:                                                                                                                                                                                                   | Response-safe GCP client configuration. Refreshable source credentials and<br/>service endpoint overrides cannot be represented by this type.                                                                        |
| `expiresAt`                                                                                                                                                                                                          | *string*                                                                                                                                                                                                             | :heavy_check_mark:                                                                                                                                                                                                   | N/A                                                                                                                                                                                                                  |
| `service`                                                                                                                                                                                                            | *"sandbox-gcp-agent-platform"*                                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                                                                   | N/A                                                                                                                                                                                                                  |