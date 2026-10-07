# ManagerInfoResponse

## Example Usage

```typescript
import { ManagerInfoResponse } from "@alienplatform/manager-api/models";

let value: ManagerInfoResponse = {
  capabilities: {
    awsSetupNodeIdentity: false,
    charts: true,
    tunnels: true,
  },
  registryHost: "<value>",
  url: "https://gullible-wheel.name/",
  version: "<value>",
};
```

## Fields

| Field                                                                                               | Type                                                                                                | Required                                                                                            | Description                                                                                         |
| --------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| `bundleSigningKey`                                                                                  | *string*                                                                                            | :heavy_minus_sign:                                                                                  | Public key (`ed25519:<base64>`) that air-gapped environments verify<br/>bundles from this manager with. |
| `capabilities`                                                                                      | [models.ManagerCapabilities](../models/managercapabilities.md)                                      | :heavy_check_mark:                                                                                  | N/A                                                                                                 |
| `operatorImage`                                                                                     | *string*                                                                                            | :heavy_minus_sign:                                                                                  | Operator image the charts this manager serves install.                                              |
| `registryHost`                                                                                      | *string*                                                                                            | :heavy_check_mark:                                                                                  | Registry host (`host[:port]`) release images and charts are pulled from.                            |
| `url`                                                                                               | *string*                                                                                            | :heavy_check_mark:                                                                                  | Public URL deployments and customers use to reach this manager.                                     |
| `version`                                                                                           | *string*                                                                                            | :heavy_check_mark:                                                                                  | Manager version.                                                                                    |