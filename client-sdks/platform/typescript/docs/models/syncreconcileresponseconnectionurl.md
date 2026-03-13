# SyncReconcileResponseConnectionUrl

## Example Usage

```typescript
import { SyncReconcileResponseConnectionUrl } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseConnectionUrl = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                    | [models.SyncReconcileResponseConnectionUrlSecretRef](../models/syncreconcileresponseconnectionurlsecretref.md) | :heavy_check_mark:                                                                                             | Reference to a Kubernetes Secret                                                                               |