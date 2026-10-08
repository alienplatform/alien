# DeploymentSetupStackSettingsPolicyDomainsKubernetes

## Example Usage

```typescript
import { DeploymentSetupStackSettingsPolicyDomainsKubernetes } from "@alienplatform/platform-api/models";

let value: DeploymentSetupStackSettingsPolicyDomainsKubernetes = {
  tlsSecretRef: {
    secretName: "<value>",
  },
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `tlsSecretRef`                                                                                                       | [models.DeploymentSetupStackSettingsPolicyTlsSecretRef](../models/deploymentsetupstacksettingspolicytlssecretref.md) | :heavy_check_mark:                                                                                                   | Namespace-scoped Kubernetes TLS Secret reference.                                                                    |