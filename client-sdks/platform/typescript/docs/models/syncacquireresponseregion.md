# SyncAcquireResponseRegion

## Example Usage

```typescript
import { SyncAcquireResponseRegion } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseRegion = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                  | [models.SyncAcquireResponseRegionSecretRef](../models/syncacquireresponseregionsecretref.md) | :heavy_check_mark:                                                                           | Reference to a Kubernetes Secret                                                             |