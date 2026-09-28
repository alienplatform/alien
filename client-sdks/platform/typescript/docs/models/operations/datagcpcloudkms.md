# DataGcpCloudKms

## Example Usage

```typescript
import { DataGcpCloudKms } from "@alienplatform/platform-api/models/operations";

let value: DataGcpCloudKms = {
  data: {
    cryptoKeyName: "<value>",
    purpose: "<value>",
    status: {
      health: "degraded",
      lifecycle: "updating",
    },
  },
  provider: "gcp-cloud-kms",
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `data`                                                                                                     | [operations.GetResourceDeploymentDetailData7](../../models/operations/getresourcedeploymentdetaildata7.md) | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `provider`                                                                                                 | *"gcp-cloud-kms"*                                                                                          | :heavy_check_mark:                                                                                         | N/A                                                                                                        |