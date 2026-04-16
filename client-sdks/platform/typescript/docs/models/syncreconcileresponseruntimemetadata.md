# SyncReconcileResponseRuntimeMetadata

Runtime metadata for deployment

Stores deployment state that needs to persist across step calls.

## Example Usage

```typescript
import { SyncReconcileResponseRuntimeMetadata } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseRuntimeMetadata = {};
```

## Fields

| Field                                                                                                                                              | Type                                                                                                                                               | Required                                                                                                                                           | Description                                                                                                                                        |
| -------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| `lastSyncedEnvVarsHash`                                                                                                                            | *string*                                                                                                                                           | :heavy_minus_sign:                                                                                                                                 | Hash of the environment variables snapshot that was last synced to the vault<br/>Used to avoid redundant sync operations during incremental deployment |
| `preparedStack`                                                                                                                                    | *models.SyncReconcileResponsePreparedStackUnion*                                                                                                   | :heavy_minus_sign:                                                                                                                                 | N/A                                                                                                                                                |