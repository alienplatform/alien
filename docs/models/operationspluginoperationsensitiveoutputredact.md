# OperationsPluginOperationSensitiveOutputRedact

## Example Usage

```typescript
import { OperationsPluginOperationSensitiveOutputRedact } from "@alienplatform/platform-api/models";

let value: OperationsPluginOperationSensitiveOutputRedact = {
  kind: "redact",
  fields: [
    "<value 1>",
  ],
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `kind`             | *"redact"*         | :heavy_check_mark: | N/A                |
| `fields`           | *string*[]         | :heavy_check_mark: | N/A                |