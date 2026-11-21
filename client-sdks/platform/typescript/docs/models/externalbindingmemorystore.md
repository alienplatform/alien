# ExternalBindingMemorystore

GCP Memorystore binding.

## Example Usage

```typescript
import { ExternalBindingMemorystore } from "@alienplatform/platform-api/models";

let value: ExternalBindingMemorystore = {
  host: "scared-vein.com",
  instanceName: "<value>",
  port: "<value>",
  serverCaCertificates: "<value>",
  username: "Saul16",
  service: "memorystore",
  type: "redis",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `host`                                                                                                               | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `instanceName`                                                                                                       | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `port`                                                                                                               | *models.PortUnion8*                                                                                                  | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `serverCaCertificates`                                                                                               | *models.ServerCaCertificatesUnion2*                                                                                  | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `username`                                                                                                           | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"memorystore"*                                                                                                      | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeRedis2](../models/typeredis2.md)                                                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |