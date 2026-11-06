# MetricsSourceScrape

## Example Usage

```typescript
import { MetricsSourceScrape } from "@alienplatform/manager-api/models";

let value: MetricsSourceScrape = {
  config: {
    addresses: [
      "<value 1>",
      "<value 2>",
    ],
    maxResponseBytes: 355970,
    maxSamples: 159680,
    replicaId: "<id>",
    timeoutSeconds: 939496,
    url: "https://other-marathon.name/",
  },
  type: "scrape",
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `config`                                             | [models.ScrapeEndpoint](../models/scrapeendpoint.md) | :heavy_check_mark:                                   | N/A                                                  |
| `type`                                               | *"scrape"*                                           | :heavy_check_mark:                                   | N/A                                                  |
