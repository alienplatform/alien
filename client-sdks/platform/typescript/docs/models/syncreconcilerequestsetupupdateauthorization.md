# SyncReconcileRequestSetupUpdateAuthorization

One-shot authority for a setup re-import to replace setup-owned resources.

## Example Usage

```typescript
import { SyncReconcileRequestSetupUpdateAuthorization } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestSetupUpdateAuthorization = {
  baselineFrozenDigest: "<value>",
  nonce: "<value>",
  releaseId: "<id>",
  setupFingerprint: "<value>",
  setupFingerprintVersion: 809897,
  setupTarget: "<value>",
  targetFrozenDigest: "<value>",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `baselineFrozenDigest`                                                   | *string*                                                                 | :heavy_check_mark:                                                       | Frozen resource projection from the last successful deployment.          |
| `nonce`                                                                  | *string*                                                                 | :heavy_check_mark:                                                       | Unique revision used by persistence layers for compare-and-swap updates. |
| `releaseId`                                                              | *string*                                                                 | :heavy_check_mark:                                                       | Release whose stack was prepared by setup.                               |
| `setupFingerprint`                                                       | *string*                                                                 | :heavy_check_mark:                                                       | Exact setup artifact revision that authored this authority.              |
| `setupFingerprintVersion`                                                | *number*                                                                 | :heavy_check_mark:                                                       | Setup fingerprint contract version.                                      |
| `setupTarget`                                                            | *string*                                                                 | :heavy_check_mark:                                                       | Stable setup target recorded on the imported deployment.                 |
| `targetFrozenDigest`                                                     | *string*                                                                 | :heavy_check_mark:                                                       | Frozen resource projection prepared by the setup re-import.              |
