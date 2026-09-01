# Retry

The operation's declared retry policy, echoed so the caller's poll loop doesn't need its own copy.

## Example Usage

```typescript
import { Retry } from "@alienplatform/platform-api/models";

let value: Retry = {
  maxAttempts: 467396,
  intervalSeconds: 597304,
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `maxAttempts`      | *number*           | :heavy_check_mark: | N/A                |
| `intervalSeconds`  | *number*           | :heavy_check_mark: | N/A                |