# GcpCredentialsServiceAccountKey

Use a full Service Account JSON key (as string). A short-lived JWT will
be created and exchanged for a bearer token automatically.

## Example Usage

```typescript
import { GcpCredentialsServiceAccountKey } from "@alienplatform/manager-api/models";

let value: GcpCredentialsServiceAccountKey = {
  json: "{key: 7023388629692829, key1: null, key2: \"<value>\"}",
  type: "serviceAccountKey",
};
```

## Fields

| Field                 | Type                  | Required              | Description           |
| --------------------- | --------------------- | --------------------- | --------------------- |
| `json`                | *string*              | :heavy_check_mark:    | N/A                   |
| `type`                | *"serviceAccountKey"* | :heavy_check_mark:    | N/A                   |