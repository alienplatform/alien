# SensitiveOutputRedact

## Example Usage

```typescript
import { SensitiveOutputRedact } from "@alienplatform/platform-api/models/operations";

let value: SensitiveOutputRedact = {
  kind: "redact",
  fields: [],
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `kind`             | *"redact"*         | :heavy_check_mark: | N/A                |
| `fields`           | *string*[]         | :heavy_check_mark: | N/A                |