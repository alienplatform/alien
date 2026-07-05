# SyncAcquireResponsePasswordSecretName

## Example Usage

```typescript
import { SyncAcquireResponsePasswordSecretName } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponsePasswordSecretName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                          | [models.SyncAcquireResponsePasswordSecretNameSecretRef](../models/syncacquireresponsepasswordsecretnamesecretref.md) | :heavy_check_mark:                                                                                                   | Reference to a Kubernetes Secret                                                                                     |