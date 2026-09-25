# GetSecretRequest

## Example Usage

```typescript
import { GetSecretRequest } from "@alienplatform/manager-api/models/operations";

let value: GetSecretRequest = {
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
