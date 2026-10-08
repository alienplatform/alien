# ExternalBindingCloudSQL

GCP Cloud SQL binding.

## Example Usage

```typescript
import { ExternalBindingCloudSQL } from "@alienplatform/platform-api/models";

let value: ExternalBindingCloudSQL = {
  database: "<value>",
  host: "political-sustenance.biz",
  passwordSecretName: null,
  port: {
    "secretRef": {
      "key": "<key>",
      "name": "<value>",
    },
  },
  serverCaCertificates: null,
  username: "Stephen_Mitchell69",
  service: "cloud-sql",
  type: "postgres",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `database`                                                                                                           | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `host`                                                                                                               | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `passwordSecretName`                                                                                                 | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `port`                                                                                                               | *models.PortUnion2*                                                                                                  | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `serverCaCertificates`                                                                                               | *models.ServerCaCertificatesUnion*                                                                                   | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `username`                                                                                                           | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"cloud-sql"*                                                                                                        | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypePostgres2](../models/typepostgres2.md)                                                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
