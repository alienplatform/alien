# RedisMetricsSample

One sample of the numbers that matter for a Redis server. A field is `None`
when the cloud did not return that metric in the window.

## Example Usage

```typescript
import { RedisMetricsSample } from "@alienplatform/manager-api/models";

let value: RedisMetricsSample = {};
```

## Fields

| Field                                                          | Type                                                           | Required                                                       | Description                                                    |
| -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- |
| `connectedClients`                                             | *number*                                                       | :heavy_minus_sign:                                             | Connected clients at the end of the window.                    |
| `evictions`                                                    | *number*                                                       | :heavy_minus_sign:                                             | Keys evicted during the sampled window.                        |
| `memoryUsedPercent`                                            | *number*                                                       | :heavy_minus_sign:                                             | Memory used as a percentage of the configured maximum (0-100). |
| `replicationLagSeconds`                                        | *number*                                                       | :heavy_minus_sign:                                             | Replication lag in seconds, when a replica exists.             |
