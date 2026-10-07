# SecretReference

Reference to a Kubernetes Secret

## Example Usage

```typescript
import { SecretReference } from "@alienplatform/manager-api/models";

let value: SecretReference = {
  key: "<key>",
  name: "<value>",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `key`              | *string*           | :heavy_check_mark: | N/A                |
| `name`             | *string*           | :heavy_check_mark: | N/A                |