# ExternalBindingBlob

Azure Blob Storage binding configuration

## Example Usage

```typescript
import { ExternalBindingBlob } from "@alienplatform/platform-api/models";

let value: ExternalBindingBlob = {
  accountName: {
    "secretRef": {
      "key": "<key>",
      "name": "<value>",
    },
  },
  containerName: "<value>",
  service: "blob",
  type: "storage",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `accountName`                                                                                                        | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `containerName`                                                                                                      | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"blob"*                                                                                                             | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeStorage2](../models/typestorage2.md)                                                                     | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
