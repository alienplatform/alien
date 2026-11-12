# IdentityRotationRequest

A client saves its generated successor before the atomic hash replacement.

## Example Usage

```typescript
import { IdentityRotationRequest } from "@alienplatform/manager-api/models";

let value: IdentityRotationRequest = {
  deploymentId: "<id>",
  key: "<key>",
  successorHash: "<value>",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `deploymentId`     | *string*           | :heavy_check_mark: | N/A                |
| `key`              | *string*           | :heavy_check_mark: | N/A                |
| `successorHash`    | *string*           | :heavy_check_mark: | N/A                |
