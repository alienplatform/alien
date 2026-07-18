# ClientConfigManual

Manual configuration with explicit values

## Example Usage

```typescript
import { ClientConfigManual } from "@alienplatform/manager-api/models";

let value: ClientConfigManual = {
  additionalHeaders: {
    "key": "<value>",
    "key1": "<value>",
  },
  mode: "manual",
  serverUrl: "https://grounded-embossing.info",
  platform: "kubernetes",
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `additionalHeaders`                                                                    | Record<string, *string*>                                                               | :heavy_check_mark:                                                                     | Additional headers to include in requests                                              |
| `certificateAuthorityData`                                                             | *string*                                                                               | :heavy_minus_sign:                                                                     | The cluster certificate authority data (base64 encoded)                                |
| `clientCertificateData`                                                                | *string*                                                                               | :heavy_minus_sign:                                                                     | Client certificate data (base64 encoded) for mutual TLS                                |
| `clientKeyData`                                                                        | *string*                                                                               | :heavy_minus_sign:                                                                     | Client key data (base64 encoded) for mutual TLS                                        |
| `insecureSkipTlsVerify`                                                                | *boolean*                                                                              | :heavy_minus_sign:                                                                     | Skip TLS verification (insecure)                                                       |
| `mode`                                                                                 | *"manual"*                                                                             | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `namespace`                                                                            | *string*                                                                               | :heavy_minus_sign:                                                                     | The namespace to operate in                                                            |
| `password`                                                                             | *string*                                                                               | :heavy_minus_sign:                                                                     | Password for basic authentication                                                      |
| `serverUrl`                                                                            | *string*                                                                               | :heavy_check_mark:                                                                     | The Kubernetes cluster server URL                                                      |
| `token`                                                                                | *string*                                                                               | :heavy_minus_sign:                                                                     | Bearer token for authentication                                                        |
| `username`                                                                             | *string*                                                                               | :heavy_minus_sign:                                                                     | Username for basic authentication                                                      |
| `platform`                                                                             | [models.ClientConfigPlatformKubernetes3](../models/clientconfigplatformkubernetes3.md) | :heavy_check_mark:                                                                     | N/A                                                                                    |