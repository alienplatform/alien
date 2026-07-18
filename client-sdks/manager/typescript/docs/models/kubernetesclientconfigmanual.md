# KubernetesClientConfigManual

Manual configuration with explicit values

## Example Usage

```typescript
import { KubernetesClientConfigManual } from "@alienplatform/manager-api/models";

let value: KubernetesClientConfigManual = {
  additionalHeaders: {},
  mode: "manual",
  serverUrl: "https://buzzing-t-shirt.biz",
};
```

## Fields

| Field                                                   | Type                                                    | Required                                                | Description                                             |
| ------------------------------------------------------- | ------------------------------------------------------- | ------------------------------------------------------- | ------------------------------------------------------- |
| `additionalHeaders`                                     | Record<string, *string*>                                | :heavy_check_mark:                                      | Additional headers to include in requests               |
| `certificateAuthorityData`                              | *string*                                                | :heavy_minus_sign:                                      | The cluster certificate authority data (base64 encoded) |
| `clientCertificateData`                                 | *string*                                                | :heavy_minus_sign:                                      | Client certificate data (base64 encoded) for mutual TLS |
| `clientKeyData`                                         | *string*                                                | :heavy_minus_sign:                                      | Client key data (base64 encoded) for mutual TLS         |
| `insecureSkipTlsVerify`                                 | *boolean*                                               | :heavy_minus_sign:                                      | Skip TLS verification (insecure)                        |
| `mode`                                                  | *"manual"*                                              | :heavy_check_mark:                                      | N/A                                                     |
| `namespace`                                             | *string*                                                | :heavy_minus_sign:                                      | The namespace to operate in                             |
| `password`                                              | *string*                                                | :heavy_minus_sign:                                      | Password for basic authentication                       |
| `serverUrl`                                             | *string*                                                | :heavy_check_mark:                                      | The Kubernetes cluster server URL                       |
| `token`                                                 | *string*                                                | :heavy_minus_sign:                                      | Bearer token for authentication                         |
| `username`                                              | *string*                                                | :heavy_minus_sign:                                      | Username for basic authentication                       |