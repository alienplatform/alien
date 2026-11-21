# Bucket

## Example Usage

```typescript
import { Bucket } from "@alienplatform/platform-api/models";

let value: Bucket = {
  start: new Date("2026-05-08T05:56:46.857Z"),
  reporting: 1431.67,
  metricsReporting: 7946.56,
  requestsPerSecond: 1649.68,
  status5xxPerSecond: 1584.84,
};
```

## Fields

| Field                                                                                                   | Type                                                                                                    | Required                                                                                                | Description                                                                                             |
| ------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| `start`                                                                                                 | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)           | :heavy_check_mark:                                                                                      | N/A                                                                                                     |
| `reporting`                                                                                             | *number*                                                                                                | :heavy_check_mark:                                                                                      | Deployments with heartbeat history in the bucket (deployments on machine-backed compute record history) |
| `metricsReporting`                                                                                      | *number*                                                                                                | :heavy_check_mark:                                                                                      | Of those, how many measured requests; the rates sum only these                                          |
| `requestsPerSecond`                                                                                     | *number*                                                                                                | :heavy_check_mark:                                                                                      | N/A                                                                                                     |
| `status5xxPerSecond`                                                                                    | *number*                                                                                                | :heavy_check_mark:                                                                                      | N/A                                                                                                     |