# RemoteSqsQueueBinding

Concrete send-only queue topology returned to remote clients.

## Example Usage

```typescript
import { RemoteSqsQueueBinding } from "@alienplatform/manager-api/models";

let value: RemoteSqsQueueBinding = {
  queueUrl: "https://triangular-casement.net",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `queueUrl`         | *string*           | :heavy_check_mark: | N/A                |