# SyncAcquireResponseEnvironmentName

## Example Usage

```typescript
import { SyncAcquireResponseEnvironmentName } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseEnvironmentName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                    | [models.SyncAcquireResponseEnvironmentNameSecretRef](../models/syncacquireresponseenvironmentnamesecretref.md) | :heavy_check_mark:                                                                                             | Reference to a Kubernetes Secret                                                                               |