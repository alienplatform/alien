# PersistImportedDeploymentRequestCustomDomains

Custom domain configuration for a single resource.

## Example Usage

```typescript
import { PersistImportedDeploymentRequestCustomDomains } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestCustomDomains = {
  certificate: {},
  domain: "live-formation.name",
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `certificate`                                                                                                                | [models.PersistImportedDeploymentRequestDomainsCertificate](../models/persistimporteddeploymentrequestdomainscertificate.md) | :heavy_check_mark:                                                                                                           | Platform-specific certificate references for custom domains.                                                                 |
| `domain`                                                                                                                     | *string*                                                                                                                     | :heavy_check_mark:                                                                                                           | Fully qualified domain name to use.                                                                                          |