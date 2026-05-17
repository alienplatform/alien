# ResourceCounts

## Example Usage

```typescript
import { ResourceCounts } from "@alienplatform/platform-api/models";

let value: ResourceCounts = {
  workers: 283825,
  containers: 575319,
  externalInfra: 747214,
  total: 577157,
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `workers`                                                                                            | *number*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `containers`                                                                                         | *number*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `externalInfra`                                                                                      | *number*                                                                                             | :heavy_check_mark:                                                                                   | Storage, queue, KV, vault, database, or cache resources that Kubernetes needs Terraform to provision |
| `total`                                                                                              | *number*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |