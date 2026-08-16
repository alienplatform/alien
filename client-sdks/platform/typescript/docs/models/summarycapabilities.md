# SummaryCapabilities

## Example Usage

```typescript
import { SummaryCapabilities } from "@alienplatform/platform-api/models";

let value: SummaryCapabilities = {
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
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `models`                                               | [models.SummaryModels](../models/summarymodels.md)     | :heavy_check_mark:                                     | N/A                                                    |
| `keys`                                                 | [models.SummaryKeys](../models/summarykeys.md)         | :heavy_check_mark:                                     | N/A                                                    |
| `buckets`                                              | [models.SummaryBuckets](../models/summarybuckets.md)   | :heavy_check_mark:                                     | N/A                                                    |
| `registry`                                             | [models.SummaryRegistry](../models/summaryregistry.md) | :heavy_check_mark:                                     | N/A                                                    |