# RuntimeIdentityProofSession

A bounded parent session; it cannot renew itself.

## Example Usage

```typescript
import { RuntimeIdentityProofSession } from "@alienplatform/manager-api/models";

let value: RuntimeIdentityProofSession = {
  token: "<value>",
  type: "session",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `token`            | *string*           | :heavy_check_mark: | N/A                |
| `type`             | *"session"*        | :heavy_check_mark: | N/A                |
