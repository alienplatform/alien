# MetricsSource

An explicitly approved collection source. Provider IAM authorization remains separate.


## Supported Types

### `models.MetricsSourceAws`

```typescript
const value: models.MetricsSourceAws = {
  config: [
    {
      dimensions: {},
      metricName: "<value>",
      namespace: "<value>",
      periodSeconds: 767028,
      statistic: "<value>",
      unit: "kilogram",
    },
  ],
  type: "aws",
};
```

### `models.MetricsSourceGcp`

```typescript
const value: models.MetricsSourceGcp = {
  config: {
    filter: "<value>",
    projectId: "<id>",
  },
  type: "gcp",
};
```

### `models.MetricsSourceAzure`

```typescript
const value: models.MetricsSourceAzure = {
  config: {
    aggregation: "<value>",
    intervalSeconds: 644918,
    metricNames: [
      "<value 1>",
      "<value 2>",
    ],
    metricNamespace: "<value>",
    resourceUri: "https://unrealistic-discourse.biz/",
  },
  type: "azure",
};
```

### `models.MetricsSourceScrape`

```typescript
const value: models.MetricsSourceScrape = {
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
