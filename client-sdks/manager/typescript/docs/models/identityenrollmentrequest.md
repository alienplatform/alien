# IdentityEnrollmentRequest

A client must durably save the corresponding key before sending this request.

## Example Usage

```typescript
import { IdentityEnrollmentRequest } from "@alienplatform/manager-api/models";

let value: IdentityEnrollmentRequest = {
  code: "<value>",
  initialKeyHash: "<value>",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `code`             | *string*           | :heavy_check_mark: | N/A                |
| `initialKeyHash`   | *string*           | :heavy_check_mark: | N/A                |
