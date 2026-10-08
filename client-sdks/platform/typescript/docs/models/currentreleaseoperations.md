# CurrentReleaseOperations

Operations declared by a stack, or by an Operator installed on its own.

## Example Usage

```typescript
import { CurrentReleaseOperations } from "@alienplatform/platform-api/models";

let value: CurrentReleaseOperations = {};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `custom`                                                                           | [models.CurrentReleaseCustom](../models/currentreleasecustom.md)[]                 | :heavy_minus_sign:                                                                 | Published custom plugins, pinned to exact versions.                                |
| `plugins`                                                                          | Record<string, [models.CurrentReleasePlugins](../models/currentreleaseplugins.md)> | :heavy_minus_sign:                                                                 | Built-in plugins, by plugin name.                                                  |