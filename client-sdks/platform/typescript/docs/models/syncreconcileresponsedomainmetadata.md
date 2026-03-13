# SyncReconcileResponseDomainMetadata

Domain metadata for auto-managed public resources (no private keys).

## Example Usage

```typescript
import { SyncReconcileResponseDomainMetadata } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseDomainMetadata = {
  baseDomain: "<value>",
  hostedZoneId: "<id>",
  publicSubdomain: "<value>",
  resources: {},
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `baseDomain`                                                                                       | *string*                                                                                           | :heavy_check_mark:                                                                                 | Base domain for auto-generated domains (e.g., "vpc.direct").                                       |
| `hostedZoneId`                                                                                     | *string*                                                                                           | :heavy_check_mark:                                                                                 | Hosted zone ID for DNS records.                                                                    |
| `publicSubdomain`                                                                                  | *string*                                                                                           | :heavy_check_mark:                                                                                 | Agent public subdomain (e.g., "k8f2j3").                                                           |
| `resources`                                                                                        | Record<string, [models.DomainMetadataTargetResources](../models/domainmetadatatargetresources.md)> | :heavy_check_mark:                                                                                 | Metadata per resource ID.                                                                          |