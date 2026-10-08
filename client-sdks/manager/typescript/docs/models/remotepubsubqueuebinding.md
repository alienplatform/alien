# RemotePubsubQueueBinding

Concrete send-only queue topology returned to remote clients.

## Example Usage

```typescript
import { RemotePubsubQueueBinding } from "@alienplatform/manager-api/models";

let value: RemotePubsubQueueBinding = {
  subscription: "<value>",
  topic: "<value>",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `subscription`     | *string*           | :heavy_check_mark: | N/A                |
| `topic`            | *string*           | :heavy_check_mark: | N/A                |