# ExternalBindingAzureManagedRedis

Azure Managed Redis binding.

## Example Usage

```typescript
import { ExternalBindingAzureManagedRedis } from "@alienplatform/platform-api/models";

let value: ExternalBindingAzureManagedRedis = {
  databaseResourceId: "<id>",
  host: "spirited-pension.net",
  port: "<value>",
  username: "Enos71",
  service: "azure-managed-redis",
  type: "redis",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `databaseResourceId`                                                                                                 | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `host`                                                                                                               | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `port`                                                                                                               | *models.PortUnion9*                                                                                                  | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `username`                                                                                                           | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"azure-managed-redis"*                                                                                              | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeRedis3](../models/typeredis3.md)                                                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |