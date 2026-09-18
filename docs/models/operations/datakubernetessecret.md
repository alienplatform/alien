# DataKubernetesSecret

## Example Usage

```typescript
import { DataKubernetesSecret } from "@alienplatform/platform-api/models/operations";

let value: DataKubernetesSecret = {
  namespace: "<value>",
  prefix: "<value>",
  secretMetadataListed: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "api-unavailable",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "deleting",
    partial: false,
    stale: false,
  },
  backend: "kubernetesSecret",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `namespace`                                                        | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `prefix`                                                           | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `secretMetadataListed`                                             | *boolean*                                                          | :heavy_check_mark:                                                 | N/A                                                                |
| `status`                                                           | [operations.DataStatus40](../../models/operations/datastatus40.md) | :heavy_check_mark:                                                 | N/A                                                                |
| `backend`                                                          | *"kubernetesSecret"*                                               | :heavy_check_mark:                                                 | N/A                                                                |