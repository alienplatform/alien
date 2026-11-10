# ExternalBindingAzureManagedRedis

Azure Managed Redis (private endpoint host + database resource ID).

## Example Usage

```typescript
import { ExternalBindingAzureManagedRedis } from "@alienplatform/manager-api/models";

let value: ExternalBindingAzureManagedRedis = {
  databaseResourceId: "<id>",
  host: "questionable-bug.net",
  port: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  username: "Elwyn.Wisozk",
  service: "azure-managed-redis",
  type: "redis",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `databaseResourceId`                                                                                                 | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `host`                                                                                                               | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `port`                                                                                                               | *models.BindingValueU16Union*                                                                                        | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `username`                                                                                                           | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"azure-managed-redis"*                                                                                              | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeRedis3](../models/typeredis3.md)                                                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
