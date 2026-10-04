# RemoteServiceBusQueueBinding

Concrete send-only queue topology returned to remote clients.

## Example Usage

```typescript
import { RemoteServiceBusQueueBinding } from "@alienplatform/manager-api/models";

let value: RemoteServiceBusQueueBinding = {
  namespace: "<value>",
  queueName: "<value>",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `namespace`        | *string*           | :heavy_check_mark: | N/A                |
| `queueName`        | *string*           | :heavy_check_mark: | N/A                |