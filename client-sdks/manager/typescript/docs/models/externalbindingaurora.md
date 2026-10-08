# ExternalBindingAurora

AWS Aurora Serverless v2 (cluster endpoint + secret ARN).

## Example Usage

```typescript
import { ExternalBindingAurora } from "@alienplatform/manager-api/models";

let value: ExternalBindingAurora = {
  clusterEndpoint: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  database: "<value>",
  passwordSecretArn: "<value>",
  port: "<value>",
  username: "Isai.Beatty57",
  service: "aurora",
  type: "postgres",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `clusterEndpoint`                                                                                                    | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `database`                                                                                                           | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `passwordSecretArn`                                                                                                  | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `port`                                                                                                               | *models.BindingValueU16Union*                                                                                        | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `username`                                                                                                           | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"aurora"*                                                                                                           | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypePostgres1](../models/typepostgres1.md)                                                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |