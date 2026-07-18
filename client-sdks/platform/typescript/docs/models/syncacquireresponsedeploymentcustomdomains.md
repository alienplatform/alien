# SyncAcquireResponseDeploymentCustomDomains

Custom domain configuration for a single resource.

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentCustomDomains } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentCustomDomains = {
  certificate: {},
  domain: "unwelcome-turret.name",
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `certificate`                                                                                                          | [models.SyncAcquireResponseDeploymentDomainsCertificate](../models/syncacquireresponsedeploymentdomainscertificate.md) | :heavy_check_mark:                                                                                                     | Platform-specific certificate references for custom domains.                                                           |
| `domain`                                                                                                               | *string*                                                                                                               | :heavy_check_mark:                                                                                                     | Fully qualified domain name to use.                                                                                    |