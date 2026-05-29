# ResourceCounts

## Example Usage

```typescript
import { ResourceCounts } from "@alienplatform/platform-api/models";

let value: ResourceCounts = {
  workers: 283825,
  containers: 575319,
  publicHttpsEndpoints: 747214,
  externalInfra: 577157,
  total: 705026,
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `workers`                                                                                            | *number*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `containers`                                                                                         | *number*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `publicHttpsEndpoints`                                                                               | *number*                                                                                             | :heavy_check_mark:                                                                                   | Workers or Containers that need managed public HTTPS endpoint setup                                  |
| `externalInfra`                                                                                      | *number*                                                                                             | :heavy_check_mark:                                                                                   | Storage, queue, KV, vault, database, or cache resources that Kubernetes needs Terraform to provision |
| `total`                                                                                              | *number*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |