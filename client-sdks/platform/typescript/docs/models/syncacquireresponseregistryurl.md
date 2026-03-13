# SyncAcquireResponseRegistryUrl

## Example Usage

```typescript
import { SyncAcquireResponseRegistryUrl } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseRegistryUrl = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                            | [models.SyncAcquireResponseRegistryUrlSecretRef](../models/syncacquireresponseregistryurlsecretref.md) | :heavy_check_mark:                                                                                     | Reference to a Kubernetes Secret                                                                       |