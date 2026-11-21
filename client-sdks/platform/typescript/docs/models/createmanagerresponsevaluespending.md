# CreateManagerResponseValuesPending

Set only by the server on a link that was created to receive values and has not been activated: the slots its client may write. It cannot create deployments.

## Example Usage

```typescript
import { CreateManagerResponseValuesPending } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseValuesPending = {
  slots: [],
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `slots`            | *string*[]         | :heavy_check_mark: | N/A                |