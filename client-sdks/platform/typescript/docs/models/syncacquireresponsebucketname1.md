# SyncAcquireResponseBucketName1

## Example Usage

```typescript
import { SyncAcquireResponseBucketName1 } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseBucketName1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                            | [models.SyncAcquireResponseBucketNameSecretRef1](../models/syncacquireresponsebucketnamesecretref1.md) | :heavy_check_mark:                                                                                     | Reference to a Kubernetes Secret                                                                       |