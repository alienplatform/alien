# RuntimeMetadata

Runtime metadata for deployment

Stores deployment state that needs to persist across step calls.

## Example Usage

```typescript
import { RuntimeMetadata } from "@alienplatform/manager-api/models";

let value: RuntimeMetadata = {
  preparedStack: {
    id: "<id>",
    resources: {
      "key": {
        config: {
          id: "<id>",
          type: "function",
        },
        dependencies: [],
        lifecycle: "live-on-setup",
      },
    },
  },
};
```

## Fields

| Field                                                                                                                                              | Type                                                                                                                                               | Required                                                                                                                                           | Description                                                                                                                                        |
| -------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| `lastSyncedEnvVarsHash`                                                                                                                            | *string*                                                                                                                                           | :heavy_minus_sign:                                                                                                                                 | Hash of the environment variables snapshot that was last synced to the vault<br/>Used to avoid redundant sync operations during incremental deployment |
| `preparedStack`                                                                                                                                    | [models.Stack](../models/stack.md)                                                                                                                 | :heavy_minus_sign:                                                                                                                                 | N/A                                                                                                                                                |