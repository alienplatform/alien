# ManagerRetryResponseValuesPending

Set only by the server on a link that was created to receive values and has not been activated: the slots its client may write. It cannot create deployments.

## Example Usage

```typescript
import { ManagerRetryResponseValuesPending } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponseValuesPending = {
  slots: [
    "<value 1>",
  ],
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `slots`            | *string*[]         | :heavy_check_mark: | N/A                |