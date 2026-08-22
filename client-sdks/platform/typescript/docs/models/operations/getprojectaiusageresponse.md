# GetProjectAiUsageResponse

Privacy-safe AI Gateway usage aggregates.


## Supported Types

### `operations.GetProjectAiUsageUnavailable`

```typescript
const value: operations.GetProjectAiUsageUnavailable = {
  status: "unavailable",
  reason: "not-configured",
};
```

### `operations.GetProjectAiUsageAvailable`

```typescript
const value: operations.GetProjectAiUsageAvailable = {
  status: "available",
  range: "24h",
  pricing: {
    label: "Estimated provider cost",
    coverage: 9612.62,
    revision: "<value>",
  },
  totals: {
    requests: 983848,
    successfulRequests: 829998,
    errorRequests: 221988,
    averageLatencyMs: null,
    p95LatencyMs: 5834.6,
    inputTokens: 119359,
    outputTokens: 938751,
    estimatedCostMicrousd: null,
    pricedRequests: 171593,
  },
  timeSeries: [],
  customers: [],
  models: [],
  customersTruncated: false,
  modelsTruncated: false,
};
```

