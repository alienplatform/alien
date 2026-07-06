# SyncReconcileResponseHorizondArtifacts

Downloadable horizond daemon artifact.

## Example Usage

```typescript
import { SyncReconcileResponseHorizondArtifacts } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseHorizondArtifacts = {
  sha256: "<value>",
  url: "https://reckless-mythology.info/",
};
```

## Fields

| Field                              | Type                               | Required                           | Description                        |
| ---------------------------------- | ---------------------------------- | ---------------------------------- | ---------------------------------- |
| `sha256`                           | *string*                           | :heavy_check_mark:                 | Expected artifact sha256 checksum. |
| `url`                              | *string*                           | :heavy_check_mark:                 | Artifact URL.                      |