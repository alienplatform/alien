# SyncAcquireResponseDeploymentDomains

Domain configuration for the stack.

When `custom_domains` is set, the specified resources use customer-provided
domains and certificates. Otherwise, Alien auto-generates domains.

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentDomains } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentDomains = {};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `customDomains`                                                                                                              | Record<string, [models.SyncAcquireResponseDeploymentCustomDomains](../models/syncacquireresponsedeploymentcustomdomains.md)> | :heavy_minus_sign:                                                                                                           | Custom domain configuration per resource ID.                                                                                 |
| `publicEndpointTarget`                                                                                                       | *models.SyncAcquireResponseDeploymentPublicEndpointTargetUnion*                                                              | :heavy_minus_sign:                                                                                                           | N/A                                                                                                                          |