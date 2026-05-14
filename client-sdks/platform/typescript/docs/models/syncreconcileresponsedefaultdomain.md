# SyncReconcileResponseDefaultDomain

## Example Usage

```typescript
import { SyncReconcileResponseDefaultDomain } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseDefaultDomain = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                    | [models.SyncReconcileResponseDefaultDomainSecretRef](../models/syncreconcileresponsedefaultdomainsecretref.md) | :heavy_check_mark:                                                                                             | Reference to a Kubernetes Secret                                                                               |