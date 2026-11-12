# ExternalBindingMemorystore

GCP Memorystore for Valkey (PSC address + CA + secret name).

## Example Usage

```typescript
import { ExternalBindingMemorystore } from "@alienplatform/manager-api/models";

let value: ExternalBindingMemorystore = {
  host: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  instanceName: "<value>",
  port: "<value>",
  serverCaCertificates: "<value>",
  username: "Edwardo42",
  service: "memorystore",
  type: "redis",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `host`                                                                                                               | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `instanceName`                                                                                                       | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `port`                                                                                                               | *models.BindingValueU16Union*                                                                                        | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `serverCaCertificates`                                                                                               | *models.BindingValueVecStringUnion*                                                                                  | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `username`                                                                                                           | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"memorystore"*                                                                                                      | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeRedis2](../models/typeredis2.md)                                                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
