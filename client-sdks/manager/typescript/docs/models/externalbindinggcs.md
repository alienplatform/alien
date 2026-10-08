# ExternalBindingGcs

Google Cloud Storage

## Example Usage

```typescript
import { ExternalBindingGcs } from "@alienplatform/manager-api/models";

let value: ExternalBindingGcs = {
  bucketName: "<value>",
  service: "gcs",
  type: "storage",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `bucketName`                                                                                                         | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"gcs"*                                                                                                              | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeStorage3](../models/typestorage3.md)                                                                     | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |