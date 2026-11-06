# ScrapeEndpoint

## Example Usage

```typescript
import { ScrapeEndpoint } from "@alienplatform/manager-api/models";

let value: ScrapeEndpoint = {
  addresses: [
    "<value 1>",
  ],
  maxResponseBytes: 366377,
  maxSamples: 31253,
  replicaId: "<id>",
  timeoutSeconds: 128824,
  url: "https://nice-testimonial.com/",
};
```

## Fields

| Field                                                                               | Type                                                                                | Required                                                                            | Description                                                                         |
| ----------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| `addresses`                                                                         | *string*[]                                                                          | :heavy_check_mark:                                                                  | Replica addresses supplied by the trusted runtime resolver; DNS is never consulted. |
| `maxResponseBytes`                                                                  | *number*                                                                            | :heavy_check_mark:                                                                  | N/A                                                                                 |
| `maxSamples`                                                                        | *number*                                                                            | :heavy_check_mark:                                                                  | N/A                                                                                 |
| `replicaId`                                                                         | *string*                                                                            | :heavy_check_mark:                                                                  | N/A                                                                                 |
| `timeoutSeconds`                                                                    | *number*                                                                            | :heavy_check_mark:                                                                  | N/A                                                                                 |
| `url`                                                                               | *string*                                                                            | :heavy_check_mark:                                                                  | N/A                                                                                 |
