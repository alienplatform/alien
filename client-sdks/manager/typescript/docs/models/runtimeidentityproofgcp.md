# RuntimeIdentityProofGcp

A metadata identity token for this manager's audience.

## Example Usage

```typescript
import { RuntimeIdentityProofGcp } from "@alienplatform/manager-api/models";

let value: RuntimeIdentityProofGcp = {
  idToken: "<value>",
  type: "gcp",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `idToken`          | *string*           | :heavy_check_mark: | N/A                |
| `type`             | *"gcp"*            | :heavy_check_mark: | N/A                |
