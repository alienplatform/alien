# SyncAcquireResponseExternalBindingsS3

AWS S3 storage binding configuration

## Example Usage

```typescript
import { SyncAcquireResponseExternalBindingsS3 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseExternalBindingsS3 = {
  service: "s3",
  type: "storage",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `bucketName`                                                                                                         | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"s3"*                                                                                                               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncAcquireResponseTypeStorage1](../models/syncacquireresponsetypestorage1.md)                               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |