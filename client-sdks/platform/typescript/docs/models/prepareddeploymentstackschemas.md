# PreparedDeploymentStackSchemas

The JSON schemas of a resource type's config and outputs.

## Example Usage

```typescript
import { PreparedDeploymentStackSchemas } from "@alienplatform/platform-api/models";

let value: PreparedDeploymentStackSchemas = {
  config: {},
  outputs: {
    "key": "<value>",
    "key1": "<value>",
    "key2": "<value>",
  },
};
```

## Fields

| Field                                 | Type                                  | Required                              | Description                           |
| ------------------------------------- | ------------------------------------- | ------------------------------------- | ------------------------------------- |
| `config`                              | Record<string, *any*>                 | :heavy_check_mark:                    | The schema of the resource's config.  |
| `outputs`                             | Record<string, *any*>                 | :heavy_check_mark:                    | The schema of the resource's outputs. |