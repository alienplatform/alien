# SyncAcquireResponseDomainMetadata

Domain metadata for auto-managed public resources (no private keys).

## Example Usage

```typescript
import { SyncAcquireResponseDomainMetadata } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseDomainMetadata = {
  baseDomain: "<value>",
  hostedZoneId: "<id>",
  publicSubdomain: "<value>",
  resources: {},
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `baseDomain`                                                                                                                 | *string*                                                                                                                     | :heavy_check_mark:                                                                                                           | Base domain for auto-generated domains (e.g., "vpc.direct").                                                                 |
| `hostedZoneId`                                                                                                               | *string*                                                                                                                     | :heavy_check_mark:                                                                                                           | Hosted zone ID for DNS records.                                                                                              |
| `publicSubdomain`                                                                                                            | *string*                                                                                                                     | :heavy_check_mark:                                                                                                           | Agent public subdomain (e.g., "k8f2j3").                                                                                     |
| `resources`                                                                                                                  | Record<string, [models.SyncAcquireResponseDomainMetadataResources](../models/syncacquireresponsedomainmetadataresources.md)> | :heavy_check_mark:                                                                                                           | Metadata per resource ID.                                                                                                    |