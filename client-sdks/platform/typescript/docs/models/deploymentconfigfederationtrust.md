# DeploymentConfigFederationTrust

The exact manager identity trusted by a deployment-owned identity pool.

## Example Usage

```typescript
import { DeploymentConfigFederationTrust } from "@alienplatform/platform-api/models";

let value: DeploymentConfigFederationTrust = {
  issuer: "mastercard",
  subject: "<value>",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `issuer`                                                                 | *string*                                                                 | :heavy_check_mark:                                                       | OIDC issuer; Google or the manager's Kubernetes issuer.                  |
| `subject`                                                                | *string*                                                                 | :heavy_check_mark:                                                       | Exact immutable Google account ID or Kubernetes service-account subject. |