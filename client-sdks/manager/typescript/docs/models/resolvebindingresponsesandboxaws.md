# ResolveBindingResponseSandboxAws

AWS Lambda MicroVM sandbox and an AWS session.

## Example Usage

```typescript
import { ResolveBindingResponseSandboxAws } from "@alienplatform/manager-api/models";

let value: ResolveBindingResponseSandboxAws = {
  binding: {
    allowEgress: false,
    imageArn: "<value>",
    imageVersion: "<value>",
    previewPorts: [
      270838,
      279654,
      152578,
    ],
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
  expiresAt: "1755710658211",
  service: "sandbox-aws",
};
```

## Fields

| Field                                                                                                                                                                                                                                                       | Type                                                                                                                                                                                                                                                        | Required                                                                                                                                                                                                                                                    | Description                                                                                                                                                                                                                                                 |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                                                                                                                                                   | [models.RemoteAwsSandboxBinding](../models/remoteawssandboxbinding.md)                                                                                                                                                                                      | :heavy_check_mark:                                                                                                                                                                                                                                          | Concrete MicroVM sandbox topology returned to remote clients.<br/><br/>Deliberately without the execution role and the egress connectors of the in-cloud binding: the<br/>provider passes no role, and a binding carrying connectors is refused before it reaches here. |
| `clientConfig`                                                                                                                                                                                                                                              | [models.RemoteAwsClientConfig](../models/remoteawsclientconfig.md)                                                                                                                                                                                          | :heavy_check_mark:                                                                                                                                                                                                                                          | Response-safe AWS client configuration. The public contract deliberately<br/>has no static, profile, metadata, or web-identity credential variants.                                                                                                         |
| `expiresAt`                                                                                                                                                                                                                                                 | *string*                                                                                                                                                                                                                                                    | :heavy_check_mark:                                                                                                                                                                                                                                          | N/A                                                                                                                                                                                                                                                         |
| `service`                                                                                                                                                                                                                                                   | *"sandbox-aws"*                                                                                                                                                                                                                                             | :heavy_check_mark:                                                                                                                                                                                                                                          | N/A                                                                                                                                                                                                                                                         |