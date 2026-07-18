# GcpCredentialsImpersonatedServiceAccount

Use a refreshable service account impersonation source.

## Example Usage

```typescript
import { GcpCredentialsImpersonatedServiceAccount } from "@alienplatform/manager-api/models";

let value: GcpCredentialsImpersonatedServiceAccount = {
  config: {
    scopes: [
      "<value 1>",
    ],
    serviceAccountEmail: "<value>",
  },
  source: {},
  type: "impersonatedServiceAccount",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `config`                                                             | [models.GcpImpersonationConfig](../models/gcpimpersonationconfig.md) | :heavy_check_mark:                                                   | Configuration for GCP service account impersonation                  |
| `source`                                                             | [models.Source](../models/source.md)                                 | :heavy_check_mark:                                                   | Source configuration used to call IAMCredentials.                    |
| `type`                                                               | *"impersonatedServiceAccount"*                                       | :heavy_check_mark:                                                   | N/A                                                                  |