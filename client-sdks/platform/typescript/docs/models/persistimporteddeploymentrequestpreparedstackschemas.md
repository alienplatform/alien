# PersistImportedDeploymentRequestPreparedStackSchemas

The JSON schemas of a resource type's config and outputs.

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPreparedStackSchemas } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestPreparedStackSchemas = {
  config: {
    "key": "<value>",
  },
  outputs: {
    "key": "<value>",
    "key1": "<value>",
  },
};
```

## Fields

| Field                                 | Type                                  | Required                              | Description                           |
| ------------------------------------- | ------------------------------------- | ------------------------------------- | ------------------------------------- |
| `config`                              | Record<string, *any*>                 | :heavy_check_mark:                    | The schema of the resource's config.  |
| `outputs`                             | Record<string, *any*>                 | :heavy_check_mark:                    | The schema of the resource's outputs. |