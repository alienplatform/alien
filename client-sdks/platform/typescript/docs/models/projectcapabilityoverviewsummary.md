# ProjectCapabilityOverviewSummary

## Example Usage

```typescript
import { ProjectCapabilityOverviewSummary } from "@alienplatform/platform-api/models";

let value: ProjectCapabilityOverviewSummary = {
  groups: 600614,
  capabilities: {
    models: {
      enabled: false,
      connected: 125192,
      settingUp: 258850,
      needsAttention: 397545,
      revoked: 804965,
      notConnected: 379194,
    },
    keys: {
      enabled: true,
      connected: 320779,
      settingUp: 72958,
      needsAttention: 143069,
      revoked: 714520,
      notConnected: 306654,
    },
    buckets: {
      enabled: false,
      connected: 546814,
      settingUp: 839265,
      needsAttention: 603306,
      revoked: 730330,
      notConnected: 924949,
    },
    registry: {
      enabled: true,
      connected: 336407,
      settingUp: 784831,
      needsAttention: 961354,
      revoked: 697113,
      notConnected: 723984,
    },
  },
};
```

## Fields

| Field                                                          | Type                                                           | Required                                                       | Description                                                    |
| -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- |
| `groups`                                                       | *number*                                                       | :heavy_check_mark:                                             | N/A                                                            |
| `capabilities`                                                 | [models.SummaryCapabilities](../models/summarycapabilities.md) | :heavy_check_mark:                                             | N/A                                                            |