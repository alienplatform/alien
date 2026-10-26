# ExternalBindingAuroraManaged

AWS Aurora Serverless v2 binding.

## Example Usage

```typescript
import { ExternalBindingAuroraManaged } from "@alienplatform/platform-api/models";

let value: ExternalBindingAuroraManaged = {
  clusterEndpoint: "<value>",
  database: "<value>",
  passwordSecretArn: {
    "secretRef": {
      "key": "<key>",
      "name": "<value>",
    },
  },
  port: {
    "secretRef": {
      "key": "<key>",
      "name": "<value>",
    },
  },
  username: {
    "secretRef": {
      "key": "<key>",
      "name": "<value>",
    },
  },
  service: "aurora-managed",
  type: "postgres",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `clusterEndpoint`                                                                                                    | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `database`                                                                                                           | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `passwordSecretArn`                                                                                                  | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `port`                                                                                                               | *models.PortUnion2*                                                                                                  | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `username`                                                                                                           | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"aurora-managed"*                                                                                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypePostgres2](../models/typepostgres2.md)                                                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |