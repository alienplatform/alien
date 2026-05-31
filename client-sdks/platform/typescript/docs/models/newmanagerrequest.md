# NewManagerRequest

## Example Usage

```typescript
import { NewManagerRequest } from "@alienplatform/platform-api/models";

let value: NewManagerRequest = {
  name: "<value>",
  cloud: "azure",
  region: "<value>",
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `name`                                                                                                                   | *string*                                                                                                                 | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `cloud`                                                                                                                  | [models.PrivateManagerCloud](../models/privatemanagercloud.md)                                                           | :heavy_check_mark:                                                                                                       | Cloud where the private manager will be deployed.                                                                        |
| `region`                                                                                                                 | *string*                                                                                                                 | :heavy_check_mark:                                                                                                       | Cloud region for the manager.                                                                                            |
| `setupMethod`                                                                                                            | [models.PrivateManagerSetupMethod](../models/privatemanagersetupmethod.md)                                               | :heavy_minus_sign:                                                                                                       | Optional setup method. Defaults to cloudformation for AWS, google-oauth for GCP, and terraform for Azure.                |
| `otlpConfig`                                                                                                             | [models.OtlpConfig](../models/otlpconfig.md)                                                                             | :heavy_minus_sign:                                                                                                       | Optional external OTLP config for forwarding logs to Axiom, Datadog, etc. Falls back to built-in DeepStore when not set. |