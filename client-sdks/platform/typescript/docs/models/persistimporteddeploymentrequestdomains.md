# PersistImportedDeploymentRequestDomains

Domain configuration for the stack.

When `custom_domains` is set, the specified resources use customer-provided
domains and certificates. Otherwise, Alien auto-generates domains.

## Example Usage

```typescript
import { PersistImportedDeploymentRequestDomains } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestDomains = {};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `customDomains`                                                                                                                    | Record<string, [models.PersistImportedDeploymentRequestCustomDomains](../models/persistimporteddeploymentrequestcustomdomains.md)> | :heavy_minus_sign:                                                                                                                 | Custom domain configuration per resource ID.                                                                                       |
| `publicEndpointTarget`                                                                                                             | *models.PersistImportedDeploymentRequestPublicEndpointTargetUnion*                                                                 | :heavy_minus_sign:                                                                                                                 | N/A                                                                                                                                |