# SyncReconcileRequestRuntimeMetadata

Runtime metadata for deployment

Stores deployment state that needs to persist across step calls.

## Example Usage

```typescript
import { SyncReconcileRequestRuntimeMetadata } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestRuntimeMetadata = {};
```

## Fields

| Field                                                                                                                                              | Type                                                                                                                                               | Required                                                                                                                                           | Description                                                                                                                                        |
| -------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| `lastSyncedEnvVarsHash`                                                                                                                            | *string*                                                                                                                                           | :heavy_minus_sign:                                                                                                                                 | Hash of the environment variables snapshot that was last synced to the vault<br/>Used to avoid redundant sync operations during incremental deployment |
| `preparedStack`                                                                                                                                    | *any*                                                                                                                                              | :heavy_minus_sign:                                                                                                                                 | N/A                                                                                                                                                |