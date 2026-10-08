# StackSettingsDomainsKubernetes

## Example Usage

```typescript
import { StackSettingsDomainsKubernetes } from "@alienplatform/platform-api/models";

let value: StackSettingsDomainsKubernetes = {
  tlsSecretRef: {
    secretName: "<value>",
  },
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `tlsSecretRef`                                                             | [models.StackSettingsTlsSecretRef](../models/stacksettingstlssecretref.md) | :heavy_check_mark:                                                         | Namespace-scoped Kubernetes TLS Secret reference.                          |