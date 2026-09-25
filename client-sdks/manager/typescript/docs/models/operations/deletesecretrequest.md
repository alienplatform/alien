# DeleteSecretRequest

## Example Usage

```typescript
import { DeleteSecretRequest } from "@alienplatform/manager-api/models/operations";

let value: DeleteSecretRequest = {
  id: "<id>",
  vaultName: "<value>",
  key: "<key>",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `id`               | *string*           | :heavy_check_mark: | Deployment ID      |
| `vaultName`        | *string*           | :heavy_check_mark: | Vault name         |
| `key`              | *string*           | :heavy_check_mark: | Secret name        |
