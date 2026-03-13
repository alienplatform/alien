# DeploymentDetailResponseRuntimeMetadata

Runtime metadata for deployment state persistence

## Example Usage

```typescript
import { DeploymentDetailResponseRuntimeMetadata } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseRuntimeMetadata = {};
```

## Fields

| Field                                                                                                                                              | Type                                                                                                                                               | Required                                                                                                                                           | Description                                                                                                                                        |
| -------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| `lastSyncedEnvVarsHash`                                                                                                                            | *string*                                                                                                                                           | :heavy_minus_sign:                                                                                                                                 | Hash of the environment variables snapshot that was last synced to the vault<br/>Used to avoid redundant sync operations during incremental deployment |
| `preparedStack`                                                                                                                                    | *any*                                                                                                                                              | :heavy_minus_sign:                                                                                                                                 | N/A                                                                                                                                                |