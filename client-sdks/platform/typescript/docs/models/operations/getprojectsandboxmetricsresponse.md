# GetProjectSandboxMetricsResponse

Privacy-safe Sandbox Gateway usage aggregates.


## Supported Types

### `operations.GetProjectSandboxMetricsUnavailable`

```typescript
const value: operations.GetProjectSandboxMetricsUnavailable = {
  status: "unavailable",
  reason: "temporarily-unavailable",
};
```

### `operations.GetProjectSandboxMetricsAvailable`

```typescript
const value: operations.GetProjectSandboxMetricsAvailable = {
  status: "available",
  range: "24h",
  totals: {
    sessions: 247824,
    commands: 998578,
    outcomeUnknown: 905229,
    errors: 59188,
    p95LatencyMs: 813.24,
  },
  timeSeries: [],
  customers: [],
  customersTruncated: false,
};
```
