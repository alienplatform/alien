# ExternalBindingLocalStorage

Local filesystem storage

## Example Usage

```typescript
import { ExternalBindingLocalStorage } from "@alienplatform/manager-api/models";

let value: ExternalBindingLocalStorage = {
  storagePath: "<value>",
  service: "local-storage",
  type: "storage",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `storagePath`                                                                                                        | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"local-storage"*                                                                                                    | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeStorage4](../models/typestorage4.md)                                                                     | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |