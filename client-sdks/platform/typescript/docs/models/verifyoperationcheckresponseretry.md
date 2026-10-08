# VerifyOperationCheckResponseRetry

The operation's declared retry policy, echoed so the caller's poll loop doesn't need its own copy.

## Example Usage

```typescript
import { VerifyOperationCheckResponseRetry } from "@alienplatform/platform-api/models";

let value: VerifyOperationCheckResponseRetry = {
  maxAttempts: 668212,
  intervalSeconds: 783692,
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `maxAttempts`      | *number*           | :heavy_check_mark: | N/A                |
| `intervalSeconds`  | *number*           | :heavy_check_mark: | N/A                |