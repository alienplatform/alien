# RedisObservedConfig

Configuration the controller reads back from the cloud, to compare with the
desired configuration.

## Example Usage

```typescript
import { RedisObservedConfig } from "@alienplatform/manager-api/models";

let value: RedisObservedConfig = {};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `authEnabled`                                                                  | *boolean*                                                                      | :heavy_minus_sign:                                                             | N/A                                                                            |
| `endpoint`                                                                     | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `engineVersion`                                                                | *string*                                                                       | :heavy_minus_sign:                                                             | Engine name and version as the cloud reports it.                               |
| `evictionPolicy`                                                               | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `nodeType`                                                                     | *string*                                                                       | :heavy_minus_sign:                                                             | Node type or SKU name.                                                         |
| `persistenceEnabled`                                                           | *boolean*                                                                      | :heavy_minus_sign:                                                             | N/A                                                                            |
| `port`                                                                         | *number*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `publicNetworkAccess`                                                          | *boolean*                                                                      | :heavy_minus_sign:                                                             | Azure only: true when the server accepts connections from the public internet. |
| `replicaCount`                                                                 | *number*                                                                       | :heavy_minus_sign:                                                             | Replicas behind the primary.                                                   |
| `tlsEnabled`                                                                   | *boolean*                                                                      | :heavy_minus_sign:                                                             | N/A                                                                            |
