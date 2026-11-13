# ExternalBindingElasticache

AWS ElastiCache for Valkey (primary endpoint + AUTH token secret ARN).

## Example Usage

```typescript
import { ExternalBindingElasticache } from "@alienplatform/manager-api/models";

let value: ExternalBindingElasticache = {
  host: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  passwordSecretArn: "<value>",
  port: 842567,
  username: "Ashley.Barrows64",
  service: "elasticache",
  type: "redis",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `host`                                                                                                               | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `passwordSecretArn`                                                                                                  | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `port`                                                                                                               | *models.BindingValueU16Union*                                                                                        | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `username`                                                                                                           | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"elasticache"*                                                                                                      | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeRedis1](../models/typeredis1.md)                                                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
