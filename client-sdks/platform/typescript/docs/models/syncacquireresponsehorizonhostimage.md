# SyncAcquireResponseHorizonHostImage

Horizon host image channel or provider-specific pointer.

Setup references these stable pointers. The concrete image version resolved
during rollout is management state, not ComputeCluster resource config.

## Example Usage

```typescript
import { SyncAcquireResponseHorizonHostImage } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseHorizonHostImage = {
  architecture: "<value>",
  channel: "<value>",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `architecture`                                                     | *string*                                                           | :heavy_check_mark:                                                 | Machine architecture, such as amd64 or arm64.                      |
| `awsSsmParameter`                                                  | *string*                                                           | :heavy_minus_sign:                                                 | AWS SSM parameter path for the channel pointer.                    |
| `azureGalleryImageDefinitionId`                                    | *string*                                                           | :heavy_minus_sign:                                                 | Azure Compute Gallery image definition ID for the channel pointer. |
| `channel`                                                          | *string*                                                           | :heavy_check_mark:                                                 | Logical image channel, such as prod, staging, or canary.           |
| `gcpImageFamily`                                                   | *string*                                                           | :heavy_minus_sign:                                                 | GCP image family for the channel pointer.                          |