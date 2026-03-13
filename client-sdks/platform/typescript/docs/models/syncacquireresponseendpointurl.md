# SyncAcquireResponseEndpointUrl

## Example Usage

```typescript
import { SyncAcquireResponseEndpointUrl } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseEndpointUrl = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                            | [models.SyncAcquireResponseEndpointUrlSecretRef](../models/syncacquireresponseendpointurlsecretref.md) | :heavy_check_mark:                                                                                     | Reference to a Kubernetes Secret                                                                       |