# SyncAcquireResponseExternalBindingsGcs

Google Cloud Storage binding configuration

## Example Usage

```typescript
import { SyncAcquireResponseExternalBindingsGcs } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseExternalBindingsGcs = {
  service: "gcs",
  type: "storage",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `bucketName`                                                                                                         | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"gcs"*                                                                                                              | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncAcquireResponseTypeStorage3](../models/syncacquireresponsetypestorage3.md)                               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |