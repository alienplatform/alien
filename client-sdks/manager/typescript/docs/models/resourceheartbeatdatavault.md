# ResourceHeartbeatDataVault

## Example Usage

```typescript
import { ResourceHeartbeatDataVault } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataVault = {
  data: {
    namespace: "<value>",
    prefix: "<value>",
    secretMetadataListed: true,
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "forbidden",
          severity: "info",
          source: "<value>",
        },
      ],
      health: "unhealthy",
      lifecycle: "scaling",
      partial: true,
      stale: true,
    },
    backend: "kubernetesSecret",
  },
  resourceType: "vault",
};
```

## Fields

| Field                       | Type                        | Required                    | Description                 |
| --------------------------- | --------------------------- | --------------------------- | --------------------------- |
| `data`                      | *models.VaultHeartbeatData* | :heavy_check_mark:          | N/A                         |
| `resourceType`              | *"vault"*                   | :heavy_check_mark:          | N/A                         |