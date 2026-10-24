# ExternalBindingLocalQueueHTTP

Connection to one manager-owned local queue. Old SDKs reject its distinct service tag.

## Example Usage

```typescript
import { ExternalBindingLocalQueueHTTP } from "@alienplatform/platform-api/models";

let value: ExternalBindingLocalQueueHTTP = {
  endpoint: "<value>",
  service: "local-queue-http",
  type: "queue",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `endpoint`                                                                                                           | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `token`                                                                                                              | *string*                                                                                                             | :heavy_minus_sign:                                                                                                   | Queue-scoped credential, injected at process start rather than persisted in stack state.                             |
| `service`                                                                                                            | *"local-queue-http"*                                                                                                 | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeQueue5](../models/typequeue5.md)                                                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |