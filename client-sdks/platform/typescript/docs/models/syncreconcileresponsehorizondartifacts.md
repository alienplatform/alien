# SyncReconcileResponseHorizondArtifacts

Download artifact for one horizond release platform.

## Example Usage

```typescript
import { SyncReconcileResponseHorizondArtifacts } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseHorizondArtifacts = {
  sha256: "<value>",
  url: "https://reckless-mythology.info/",
};
```

## Fields

| Field    | Type     | Required           | Description                              |
| -------- | -------- | ------------------ | ---------------------------------------- |
| `sha256` | _string_ | :heavy_check_mark: | SHA-256 digest for the artifact payload. |
| `url`    | _string_ | :heavy_check_mark: | HTTPS URL for the artifact.              |
