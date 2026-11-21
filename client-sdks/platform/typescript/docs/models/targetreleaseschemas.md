# TargetReleaseSchemas

The JSON schemas of a resource type's config and outputs.

## Example Usage

```typescript
import { TargetReleaseSchemas } from "@alienplatform/platform-api/models";

let value: TargetReleaseSchemas = {
  config: {
    "key": "<value>",
    "key1": "<value>",
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